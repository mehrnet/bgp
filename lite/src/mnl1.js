// Zero-copy MNL1 reader directly over a static Uint8Array / ArrayBuffer.

import { formatIPv4, formatIPv6, maskIPv4Range, maskIPv6Range } from "./ip.js";

const decoder = new TextDecoder();

function readUvarint(bytes, pos, end) {
  let x = 0;
  let s = 0;
  for (let i = 0; pos < end && i < 10; i++) {
    const b = bytes[pos++];
    if (b < 0x80) {
      return [x + b * 2 ** s, pos];
    }
    x += (b & 0x7f) * 2 ** s;
    s += 7;
  }
  return null;
}

let _uvNext = 0;

function readUv(bytes, pos, end) {
  let x = 0;
  let mul = 1;
  for (let i = 0; pos < end && i < 10; i++) {
    const b = bytes[pos++];
    if (b < 0x80) {
      _uvNext = pos;
      return x + b * mul;
    }
    x += (b & 0x7f) * mul;
    mul *= 128;
  }
  return -1;
}

function readUvarintBig(bytes, pos, end) {
  let x = 0n;
  let s = 0n;
  for (let i = 0; pos < end && i < 10; i++) {
    const b = bytes[pos++];
    if (b < 0x80) {
      return [x + (BigInt(b) << s), pos];
    }
    x += BigInt(b & 0x7f) << s;
    s += 7n;
  }
  return null;
}

function readOffsetDict(view, pos) {
  const count = view.getUint32(pos, true);
  const offsetsOff = pos + 4;
  const dataOff = offsetsOff + (count + 1) * 4;
  const totalBytes = view.getUint32(offsetsOff + count * 4, true);
  return [{ isOffset: true, count, offsetsOff, dataOff }, dataOff + totalBytes];
}

function readLazyDict(bytes, pos) {
  const c = readUvarint(bytes, pos, bytes.length);
  if (!c) throw new Error("bad dict count");
  const count = c[0];
  pos = c[1];
  const offs = new Uint32Array(count);
  const lens = new Uint32Array(count);
  for (let i = 0; i < count; i++) {
    const l = readUvarint(bytes, pos, bytes.length);
    if (!l) throw new Error("bad string len");
    const len = l[0];
    pos = l[1];
    offs[i] = pos;
    lens[i] = len;
    pos += len;
  }
  return [{ offs, lens }, pos];
}

function getKeyNum(bytes, off, kb) {
  let v = 0;
  for (let i = kb - 1; i >= 0; i--) {
    v = v * 256 + bytes[off + i];
  }
  return v;
}

function openSegTable(bytes, view, pos, bits, blockSize, isV6, hasShifted) {
  const entries = view.getUint32(pos, true);
  const dataLen = view.getUint32(pos + 4, true);
  pos += 8;
  const kb = (bits + 7) >> 3;
  const blocks = Math.ceil(entries / blockSize);
  const indexLen = blocks * (kb + 4);
  const shift = hasShifted ? (isV6 && bits === 64 ? 16 : !isV6 && bits === 32 ? 8 : 0) : 0;
  return [
    { bits, kb, stride: kb + 4, shift, entries, blocks, indexOff: pos, dataOff: pos + indexLen, dataLen },
    pos + indexLen + dataLen,
  ];
}

function decodeDelta(table, encoded) {
  if (table.shift === 0) return encoded;
  return (encoded & 1n) === 0n ? (encoded >> 1n) << BigInt(table.shift) : encoded >> 1n;
}

function openChunkedSection(view, pos, itemCount) {
  const chunkSize = view.getUint16(pos, true);
  pos += 2;
  const numChunks = Math.ceil(itemCount / chunkSize);
  const offsetsOff = pos;
  pos += (numChunks + 1) * 4;
  const dataOff = pos;
  const totalBytes = view.getUint32(offsetsOff + numChunks * 4, true);
  return [{ chunkSize, numChunks, offsetsOff, dataOff, cache: new Map() }, pos + totalBytes];
}

async function inflateRaw(compressed) {
  const stream = new Response(compressed).body.pipeThrough(new DecompressionStream("deflate-raw"));
  const buf = await new Response(stream).arrayBuffer();
  const bytes = new Uint8Array(buf);
  return { bytes, view: new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength) };
}

function getKeyBig(bytes, off, kb) {
  let v = 0n;
  for (let i = kb - 1; i >= 0; i--) {
    v = (v << 8n) | BigInt(bytes[off + i]);
  }
  return v;
}

export class LiteDB {
  constructor(buffer) {
    const bytes = new Uint8Array(buffer);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const ver = bytes[4];
    if (
      bytes.length < 13 ||
      bytes[0] !== 0x4d || // M
      bytes[1] !== 0x4e || // N
      bytes[2] !== 0x4c || // L
      bytes[3] !== 0x31 || // 1
      ver < 3 ||
      ver > 6
    ) {
      throw new Error("invalid MNL1 header");
    }
    this.version = ver;
    this.bytes = bytes;
    this.view = view;
    this.flags = bytes[5];
    this.blockSize = bytes[6];
    this.allocV4B = bytes[7];
    this.allocV6B = bytes[8];
    this.routeV4B = bytes[9];
    this.routeV6B = bytes[10];
    let pos = 13;
    if (ver >= 4) {
      if (bytes.length < 15) throw new Error("invalid MNL1 header");
      this.geoV4B = bytes[11];
      this.geoV6B = bytes[12];
      pos = 15;
    } else {
      this.geoV4B = this.allocV4B;
      this.geoV6B = this.allocV6B;
    }
    const metaLen = view.getUint16(pos - 2, true);
    this.meta = metaLen > 0 ? JSON.parse(decoder.decode(bytes.subarray(pos, pos + metaLen))) : {};
    pos += metaLen;

    const ccCount = view.getUint16(pos, true);
    pos += 2;
    this.countriesOff = pos;
    this.ccCount = ccCount;
    pos += 2 * ccCount;

    const readDict = ver >= 6 ? (p) => readOffsetDict(view, p) : (p) => readLazyDict(bytes, p);
    [this.registries, pos] = readDict(pos);
    [this.statuses, pos] = readDict(pos);
    [this.sources, pos] = readDict(pos);
    [this.regions, pos] = readDict(pos);
    [this.cities, pos] = readDict(pos);

    this.geoLocCount = view.getUint32(pos, true);
    pos += 4;
    this.geoLocOffsetsOff = pos;
    pos += (this.geoLocCount + 1) * 4;
    this.geoLocDataOff = pos;
    pos += view.getUint32(this.geoLocOffsetsOff + this.geoLocCount * 4, true);

    this.asnCount = view.getUint32(pos, true);
    pos += 4;
    this.asnNumbersOff = pos;
    pos += this.asnCount * 4;
    const isChunked = (this.flags & 4) !== 0;
    if (isChunked && ver < 6) {
      [this.autnumChunks, pos] = openChunkedSection(view, pos, this.asnCount);
      this.asnOffsetsOff = -1;
      this.asnDataOff = -1;
    } else {
      this.autnumChunks = null;
      this.asnOffsetsOff = pos;
      pos += (this.asnCount + 1) * 4;
      this.asnDataOff = pos;
      pos += view.getUint32(this.asnOffsetsOff + this.asnCount * 4, true);
    }

    this.multiCount = view.getUint32(pos, true);
    pos += 4;
    this.multiOffsetsOff = pos;
    pos += (this.multiCount + 1) * 4;
    this.multiDataOff = pos;
    pos += view.getUint32(this.multiOffsetsOff + this.multiCount * 4, true);

    if ((this.flags & 8) !== 0) {
      this.routeValCount = view.getUint32(pos, true);
      pos += 4;
      this.routeValOff = pos;
      pos += this.routeValCount * 4;
    } else {
      this.routeValCount = 0;
      this.routeValOff = -1;
    }

    const hasShifted = (this.flags & 2) !== 0;
    [this.allocV4, pos] = openSegTable(bytes, view, pos, this.allocV4B, this.blockSize, false, hasShifted);
    [this.allocV6, pos] = openSegTable(bytes, view, pos, this.allocV6B, this.blockSize, true, hasShifted);
    [this.geoV4, pos] = openSegTable(bytes, view, pos, this.geoV4B, this.blockSize, false, hasShifted);
    [this.geoV6, pos] = openSegTable(bytes, view, pos, this.geoV6B, this.blockSize, true, hasShifted);
    [this.routeV4, pos] = openSegTable(bytes, view, pos, this.routeV4B, this.blockSize, false, hasShifted);
    [this.routeV6, pos] = openSegTable(bytes, view, pos, this.routeV6B, this.blockSize, true, hasShifted);

    if ((this.flags & 1) !== 0) {
      if (isChunked) {
        [this.asnRouteChunks, pos] = openChunkedSection(view, pos, this.asnCount);
        this.asnRouteOffsetsOff = -1;
        this.asnRouteDataOff = -1;
      } else {
        this.asnRouteChunks = null;
        this.asnRouteOffsetsOff = pos;
        pos += (this.asnCount + 1) * 4;
        this.asnRouteDataOff = pos;
      }
    } else {
      this.asnRouteChunks = null;
      this.asnRouteOffsetsOff = -1;
      this.asnRouteDataOff = -1;
    }
  }

  async readChunkEntry(section, legacyOffsetsOff, legacyDataOff, zeroBasedIdx) {
    if (!section) {
      const off = legacyDataOff + this.view.getUint32(legacyOffsetsOff + zeroBasedIdx * 4, true);
      const end = legacyDataOff + this.view.getUint32(legacyOffsetsOff + (zeroBasedIdx + 1) * 4, true);
      return { bytes: this.bytes, off, end };
    }
    const chunkIdx = Math.floor(zeroBasedIdx / section.chunkSize);
    const localIdx = zeroBasedIdx % section.chunkSize;
    let chunk = section.cache.get(chunkIdx);
    if (!chunk) {
      const cStart = section.dataOff + this.view.getUint32(section.offsetsOff + chunkIdx * 4, true);
      const cEnd = section.dataOff + this.view.getUint32(section.offsetsOff + (chunkIdx + 1) * 4, true);
      chunk = await inflateRaw(this.bytes.subarray(cStart, cEnd));
      if (section.cache.size >= 64) {
        section.cache.delete(section.cache.keys().next().value);
      }
      section.cache.set(chunkIdx, chunk);
    }
    const off = chunk.view.getUint32(localIdx * 4, true);
    const end = chunk.view.getUint32((localIdx + 1) * 4, true);
    return { bytes: chunk.bytes, off, end };
  }

  countryAt(idx) {
    if (idx <= 0 || idx > this.ccCount) return null;
    const p = this.countriesOff + (idx - 1) * 2;
    return String.fromCharCode(this.bytes[p], this.bytes[p + 1]);
  }

  dictAt(dict, idx) {
    if (idx <= 0) return null;
    if (dict.isOffset) {
      if (idx > dict.count) return null;
      const off = dict.dataOff + this.view.getUint32(dict.offsetsOff + (idx - 1) * 4, true);
      const end = dict.dataOff + this.view.getUint32(dict.offsetsOff + idx * 4, true);
      return decoder.decode(this.bytes.subarray(off, end));
    }
    if (idx > dict.lens.length) return null;
    const off = dict.offs[idx - 1];
    return decoder.decode(this.bytes.subarray(off, off + dict.lens[idx - 1]));
  }

  resolveRouteVal(val) {
    if (val === 0 || this.routeValCount === 0) return val;
    if (val > this.routeValCount) return 0;
    return this.view.getUint32(this.routeValOff + (val - 1) * 4, true);
  }

  decodeAutnumFrom(bytes, off, end, asn, summaryOnly) {
    if (off >= end || bytes[off] === 0) {
      return { asNumber: asn, hasAutnum: false };
    }
    let p = off + 1;
    if (summaryOnly) {
      p = skipVarint(bytes, p, end);
      if (p < 0) return { asNumber: asn, hasAutnum: false };
      p = skipVarint(bytes, p, end);
      if (p < 0) return { asNumber: asn, hasAutnum: false };
      p = skipVarint(bytes, p, end);
      if (p < 0) return { asNumber: asn, hasAutnum: false };
      p = skipVarint(bytes, p, end);
      if (p < 0) return { asNumber: asn, hasAutnum: false };
      const nameLen = readUv(bytes, p, end);
      if (nameLen < 0) return { asNumber: asn, hasAutnum: false };
      p = _uvNext;
      const name = nameLen > 0 ? decoder.decode(bytes.subarray(p, p + nameLen)) : null;
      p += nameLen;
      const orgLen = readUv(bytes, p, end);
      if (orgLen < 0) return { asNumber: asn, hasAutnum: true, name, organization: null };
      p = _uvNext;
      const org = orgLen > 0 ? decoder.decode(bytes.subarray(p, p + orgLen)) : null;
      return { asNumber: asn, hasAutnum: true, name, organization: org };
    }
    const r1 = readUv(bytes, p, end);
    const r2 = readUv(bytes, _uvNext, end);
    const r3 = readUv(bytes, _uvNext, end);
    const r4 = readUv(bytes, _uvNext, end);
    const nameLen = readUv(bytes, _uvNext, end);
    p = _uvNext;
    const name = nameLen > 0 ? decoder.decode(bytes.subarray(p, p + nameLen)) : null;
    p += nameLen;
    const orgLen = readUv(bytes, p, end);
    p = _uvNext;
    const org = orgLen > 0 ? decoder.decode(bytes.subarray(p, p + orgLen)) : null;
    p += orgLen;
    const mntLen = readUv(bytes, p, end);
    p = _uvNext;
    const maintainers = mntLen > 0 ? decoder.decode(bytes.subarray(p, p + mntLen)) : null;
    p += mntLen;
    const acLen = readUv(bytes, p, end);
    p = _uvNext;
    const abuseContact = acLen > 0 ? decoder.decode(bytes.subarray(p, p + acLen)) : null;
    p += acLen;
    const descLen = readUv(bytes, p, end);
    p = _uvNext;
    const description = descLen > 0 ? decoder.decode(bytes.subarray(p, p + descLen)) : null;
    return {
      asNumber: asn,
      hasAutnum: true,
      registry: this.dictAt(this.registries, r1),
      country: this.countryAt(r2),
      status: this.dictAt(this.statuses, r3),
      source: this.dictAt(this.sources, r4),
      name,
      organization: org,
      maintainers,
      abuseContact,
      description,
    };
  }

  autnumByIdxSync(idx, summaryOnly = false) {
    if (idx < 1 || idx > this.asnCount) return { asNumber: 0, hasAutnum: false };
    const asn = this.view.getUint32(this.asnNumbersOff + (idx - 1) * 4, true);
    const off = this.asnDataOff + this.view.getUint32(this.asnOffsetsOff + (idx - 1) * 4, true);
    const end = this.asnDataOff + this.view.getUint32(this.asnOffsetsOff + idx * 4, true);
    return this.decodeAutnumFrom(this.bytes, off, end, asn, summaryOnly);
  }

  async autnumByIdx(idx, summaryOnly = false) {
    if (!this.autnumChunks) return this.autnumByIdxSync(idx, summaryOnly);
    if (idx < 1 || idx > this.asnCount) return { asNumber: 0, hasAutnum: false };
    const asn = this.view.getUint32(this.asnNumbersOff + (idx - 1) * 4, true);
    const { bytes, off, end } = await this.readChunkEntry(
      this.autnumChunks,
      this.asnOffsetsOff,
      this.asnDataOff,
      idx - 1
    );
    return this.decodeAutnumFrom(bytes, off, end, asn, summaryOnly);
  }

  resolveASNSet(val, summaryOnly = false) {
    if (val === 0) return [];
    if (val <= this.asnCount) {
      if (!this.autnumChunks) return [this.autnumByIdxSync(val, summaryOnly)];
      return this.autnumByIdx(val, summaryOnly).then((r) => [r]);
    }
    const mIdx = val - this.asnCount - 1;
    if (mIdx < 0 || mIdx >= this.multiCount) return [];
    const off = this.multiDataOff + this.view.getUint32(this.multiOffsetsOff + mIdx * 4, true);
    const end = this.multiDataOff + this.view.getUint32(this.multiOffsetsOff + (mIdx + 1) * 4, true);
    const count = readUv(this.bytes, off, end);
    if (count < 0) return [];
    let p = _uvNext;
    const indices = new Array(count);
    for (let i = 0; i < count; i++) {
      const v = readUv(this.bytes, p, end);
      if (v < 0) return [];
      indices[i] = v;
      p = _uvNext;
    }
    if (!this.autnumChunks) {
      const out = new Array(count);
      for (let i = 0; i < count; i++) {
        out[i] = this.autnumByIdxSync(indices[i], summaryOnly);
      }
      return out;
    }
    return Promise.all(indices.map((i) => this.autnumByIdx(i, summaryOnly)));
  }

  findBlock4(table, keyNum) {
    let lo = 0;
    let hi = table.blocks;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      const k = getKeyNum(this.bytes, table.indexOff + mid * table.stride, table.kb);
      if (k > keyNum) {
        hi = mid;
      } else {
        lo = mid + 1;
      }
    }
    return lo - 1;
  }

  findSeg4(table, keyNum, skipPayload) {
    const b = this.findBlock4(table, keyNum);
    if (b < 0) return null;
    let curKey = getKeyNum(this.bytes, table.indexOff + b * table.stride, table.kb);
    let off = table.dataOff + this.view.getUint32(table.indexOff + b * table.stride + table.kb, true);
    const end =
      b + 1 < table.blocks
        ? table.dataOff + this.view.getUint32(table.indexOff + (b + 1) * table.stride + table.kb, true)
        : table.dataOff + table.dataLen;

    let prevStart = 0;
    let prevPStart = 0;
    let prevPEnd = 0;
    let found = false;
    while (off < end) {
      let enc = 0;
      let mul = 1;
      let pStart = off;
      let ok = false;
      for (let i = 0; pStart < end && i < 10; i++) {
        const byte = this.bytes[pStart++];
        if (byte < 0x80) {
          enc += byte * mul;
          ok = true;
          break;
        }
        enc += (byte & 0x7f) * mul;
        mul *= 128;
      }
      if (!ok) return null;
      const delta =
        table.shift === 0
          ? enc
          : enc % 2 === 0
            ? Math.floor(enc / 2) * (1 << table.shift)
            : Math.floor(enc / 2);
      curKey += delta;
      const nextOff = skipPayload(this.bytes, pStart, end);
      if (nextOff < 0) return null;
      if (curKey > keyNum) {
        if (!found) return null;
        return { segStart: prevStart, nextStart: curKey, hasNext: true, pStart: prevPStart, pEnd: prevPEnd };
      }
      prevStart = curKey;
      prevPStart = pStart;
      prevPEnd = nextOff;
      found = true;
      off = nextOff;
    }
    if (!found) return null;
    if (b + 1 < table.blocks) {
      const nextBlockKey = getKeyNum(this.bytes, table.indexOff + (b + 1) * table.stride, table.kb);
      return { segStart: prevStart, nextStart: nextBlockKey, hasNext: true, pStart: prevPStart, pEnd: prevPEnd };
    }
    return { segStart: prevStart, nextStart: 0, hasNext: false, pStart: prevPStart, pEnd: prevPEnd };
  }

  findBlock(table, keyBig) {
    let lo = 0;
    let hi = table.blocks;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      const k = getKeyBig(this.bytes, table.indexOff + mid * table.stride, table.kb);
      if (k > keyBig) {
        hi = mid;
      } else {
        lo = mid + 1;
      }
    }
    return lo - 1;
  }

  findSeg(table, keyBig, skipPayload) {
    const b = this.findBlock(table, keyBig);
    if (b < 0) return null;
    let curKey = getKeyBig(this.bytes, table.indexOff + b * table.stride, table.kb);
    let off = table.dataOff + this.view.getUint32(table.indexOff + b * table.stride + table.kb, true);
    const end =
      b + 1 < table.blocks
        ? table.dataOff + this.view.getUint32(table.indexOff + (b + 1) * table.stride + table.kb, true)
        : table.dataOff + table.dataLen;

    let prevStart = 0n;
    let prevPStart = 0;
    let prevPEnd = 0;
    let found = false;
    while (off < end) {
      const d = readUvarintBig(this.bytes, off, end);
      if (!d) return null;
      curKey += decodeDelta(table, d[0]);
      const pStart = d[1];
      const nextOff = skipPayload(this.bytes, pStart, end);
      if (nextOff < 0) return null;
      if (curKey > keyBig) {
        if (!found) return null;
        return { segStart: prevStart, nextStart: curKey, hasNext: true, pStart: prevPStart, pEnd: prevPEnd };
      }
      prevStart = curKey;
      prevPStart = pStart;
      prevPEnd = nextOff;
      found = true;
      off = nextOff;
    }
    if (!found) return null;
    if (b + 1 < table.blocks) {
      const nextBlockKey = getKeyBig(this.bytes, table.indexOff + (b + 1) * table.stride, table.kb);
      return { segStart: prevStart, nextStart: nextBlockKey, hasNext: true, pStart: prevPStart, pEnd: prevPEnd };
    }
    return { segStart: prevStart, nextStart: 0n, hasNext: false, pStart: prevPStart, pEnd: prevPEnd };
  }

  lookupCountry(parsedIP) {
    const isV6 = parsedIP.isV6;
    const geoTable = isV6 ? this.geoV6 : this.geoV4;
    const geoKey = isV6
      ? (parsedIP.value >> 64n) >> BigInt(64 - this.geoV6B)
      : parsedIP.value >>> (32 - this.geoV4B);
    const geoSeg = isV6
      ? this.findSeg(geoTable, geoKey, skipGeo)
      : this.findSeg4(geoTable, geoKey, skipGeo);
    if (geoSeg) {
      const locID = readUv(this.bytes, geoSeg.pStart, geoSeg.pEnd);
      if (locID > 0 && locID <= this.geoLocCount) {
        const idx = locID - 1;
        const off = this.geoLocDataOff + this.view.getUint32(this.geoLocOffsetsOff + idx * 4, true);
        const end = this.geoLocDataOff + this.view.getUint32(this.geoLocOffsetsOff + (idx + 1) * 4, true);
        const ccId = readUv(this.bytes, off, end);
        if (ccId >= 0) {
          const cc = this.countryAt(ccId);
          if (cc) return cc;
        }
      }
    }

    const allocTable = isV6 ? this.allocV6 : this.allocV4;
    const allocKey = isV6
      ? (parsedIP.value >> 64n) >> BigInt(64 - this.allocV6B)
      : parsedIP.value >>> (32 - this.allocV4B);
    const allocSeg = isV6
      ? this.findSeg(allocTable, allocKey, skipAlloc)
      : this.findSeg4(allocTable, allocKey, skipAlloc);
    if (allocSeg) {
      const regId = readUv(this.bytes, allocSeg.pStart, allocSeg.pEnd);
      if (regId >= 0) {
        const ccId = readUv(this.bytes, _uvNext, allocSeg.pEnd);
        if (ccId >= 0) return this.countryAt(ccId);
      }
    }
    return null;
  }

  async lookupIP(parsedIP) {
    const isV6 = parsedIP.isV6;
    const allocTable = isV6 ? this.allocV6 : this.allocV4;
    const geoTable = isV6 ? this.geoV6 : this.geoV4;
    const routeTable = isV6 ? this.routeV6 : this.routeV4;

    const allocKey = isV6
      ? (parsedIP.value >> 64n) >> BigInt(64 - this.allocV6B)
      : parsedIP.value >>> (32 - this.allocV4B);
    const geoKey = isV6
      ? (parsedIP.value >> 64n) >> BigInt(64 - this.geoV6B)
      : parsedIP.value >>> (32 - this.geoV4B);
    const routeKey = isV6
      ? (parsedIP.value >> 64n) >> BigInt(64 - this.routeV6B)
      : parsedIP.value >>> 0;

    let hasAlloc = false;
    let registry = null;
    let allocCC = null;
    let allocStart = null;
    let allocEnd = null;
    const allocSeg = isV6
      ? this.findSeg(allocTable, allocKey, skipAlloc)
      : this.findSeg4(allocTable, allocKey, skipAlloc);
    if (allocSeg) {
      const regId = readUv(this.bytes, allocSeg.pStart, allocSeg.pEnd);
      const ccId = regId >= 0 ? readUv(this.bytes, _uvNext, allocSeg.pEnd) : -1;
      if (ccId >= 0 && (regId !== 0 || ccId !== 0)) {
        hasAlloc = true;
        registry = this.dictAt(this.registries, regId);
        allocCC = this.countryAt(ccId);
        [allocStart, allocEnd] = slotBoundsToStrings(isV6, allocTable.bits, allocSeg.segStart, allocSeg.nextStart, allocSeg.hasNext);
      }
    }

    let hasGeo = false;
    let geoCC = null;
    let geoRegion = null;
    let geoCity = null;
    const geoSeg = isV6
      ? this.findSeg(geoTable, geoKey, skipGeo)
      : this.findSeg4(geoTable, geoKey, skipGeo);
    if (geoSeg) {
      const locID = readUv(this.bytes, geoSeg.pStart, geoSeg.pEnd);
      if (locID > 0 && locID <= this.geoLocCount) {
        const idx = locID - 1;
        const off = this.geoLocDataOff + this.view.getUint32(this.geoLocOffsetsOff + idx * 4, true);
        const end = this.geoLocDataOff + this.view.getUint32(this.geoLocOffsetsOff + (idx + 1) * 4, true);
        const ccId = readUv(this.bytes, off, end);
        const regId = ccId >= 0 ? readUv(this.bytes, _uvNext, end) : -1;
        const cityId = regId >= 0 ? readUv(this.bytes, _uvNext, end) : -1;
        if (cityId >= 0) {
          hasGeo = true;
          geoCC = this.countryAt(ccId);
          geoRegion = this.dictAt(this.regions, regId);
          geoCity = this.dictAt(this.cities, cityId);
        }
      }
    }

    let hasRoute = false;
    let routeNet = null;
    let routeASNs = [];
    const routeSeg = isV6
      ? this.findSeg(routeTable, routeKey, skipRoute)
      : this.findSeg4(routeTable, routeKey, skipRoute);
    if (routeSeg && routeSeg.pStart < routeSeg.pEnd) {
      const bits = this.bytes[routeSeg.pStart];
      if (bits > 0) {
        const rVal = readUv(this.bytes, routeSeg.pStart + 1, routeSeg.pEnd);
        const rawRouteVal = rVal >= 0 ? this.resolveRouteVal(rVal) : 0;
        if (rawRouteVal !== 0) {
          hasRoute = true;
          if (isV6) {
            const { start, end, cidr } = maskIPv6Range(parsedIP.value, bits);
            routeNet = { cidr, startIP: formatIPv6(start), endIP: formatIPv6(end) };
          } else {
            const { start, end, cidr } = maskIPv4Range(parsedIP.value, bits);
            routeNet = { cidr, startIP: formatIPv4(start), endIP: formatIPv4(end) };
          }
          const resolved = this.resolveASNSet(rawRouteVal, true);
          routeASNs = this.autnumChunks ? await resolved : resolved;
        }
      }
    }

    if (!hasAlloc && !hasGeo && !hasRoute) return null;

    const asnStrings = routeASNs.map((a) => `AS${a.asNumber}`);
    const singleASN = asnStrings.length === 1;
    return {
      ip: parsedIP.canonical,
      version: isV6 ? 6 : 4,
      registry,
      allocation_date: null,
      allocation_status: null,
      network: {
        cidr: routeNet ? routeNet.cidr : null,
        start_ip: routeNet ? routeNet.startIP : null,
        end_ip: routeNet ? routeNet.endIP : null,
        asn: singleASN ? asnStrings[0] : null,
        asns: asnStrings,
        origins: routeASNs.map((a) => ({
          asn: `AS${a.asNumber}`,
          as_number: a.asNumber,
          name: a.hasAutnum ? a.name : null,
          organization: a.hasAutnum ? a.organization : null,
        })),
        as_number: singleASN ? routeASNs[0].asNumber : null,
        status: null,
        abuse_contact: null,
      },
      allocation: {
        start_ip: allocStart,
        end_ip: allocEnd,
        registry,
        country_code: allocCC,
        country_raw: allocCC,
        name: null,
        allocation_date: null,
        status: null,
        abuse_contact: null,
      },
      location: {
        country_code: geoCC || allocCC,
        region: geoRegion,
        city: geoCity,
      },
      sources: {
        allocation: hasAlloc,
        route: hasRoute,
        geofeed: hasGeo,
      },
    };
  }

  findASNIndex(asNumber) {
    let lo = 0;
    let hi = this.asnCount;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      const v = this.view.getUint32(this.asnNumbersOff + mid * 4, true);
      if (v < asNumber) lo = mid + 1;
      else hi = mid;
    }
    if (lo >= this.asnCount || this.view.getUint32(this.asnNumbersOff + lo * 4, true) !== asNumber) {
      return -1;
    }
    return lo;
  }

  async lookupASN(asNumber, offset, limit) {
    const idx = this.findASNIndex(asNumber);
    if (idx < 0) return null;
    const rec = await this.autnumByIdx(idx + 1);
    const items = [];
    let totalItems = 0;

    if ((this.flags & 1) !== 0) {
      const entry = await this.readChunkEntry(
        this.asnRouteChunks,
        this.asnRouteOffsetsOff,
        this.asnRouteDataOff,
        idx
      );
      const rBytes = entry.bytes;
      let p = entry.off;
      const end = entry.end;
      const asnStr = `AS${asNumber}`;
      while (p < end) {
        const rawLen = rBytes[p++];
        const isV6 = rawLen >= 128;
        const bits = isV6 ? rawLen - 128 : rawLen;
        const nb = (bits + 7) >> 3;
        if (p + nb > end) break;
        if (totalItems >= offset && items.length < limit) {
          let prefix;
          if (isV6) {
            let v = 0n;
            for (let i = 0; i < nb; i++) {
              v |= BigInt(rBytes[p + i]) << BigInt(8 * (15 - i));
            }
            prefix = `${formatIPv6(v)}/${bits}`;
          } else {
            let v = 0;
            for (let i = 0; i < nb; i++) {
              v |= rBytes[p + i] << (8 * (3 - i));
            }
            prefix = `${formatIPv4(v >>> 0)}/${bits}`;
          }
          items.push({
            prefix,
            version: isV6 ? 6 : 4,
            origin_asn: asnStr,
            as_number: asNumber,
            registry: null,
            source: null,
            maintainers: null,
            organization: null,
            abuse_contact: null,
            description: null,
          });
        }
        p += nb;
        totalItems++;
      }
    }

    if (!rec.hasAutnum && totalItems === 0) return null;
    const asnStr = `AS${asNumber}`;
    return {
      asn: asnStr,
      as_number: asNumber,
      autnum: rec.hasAutnum
        ? {
            asn: asnStr,
            as_number: asNumber,
            registry: rec.registry,
            country_code: rec.country,
            country_raw: rec.country,
            name: rec.name,
            organization: rec.organization,
            status: rec.status,
            created: null,
            last_modified: null,
            source: rec.source,
            maintainers: rec.maintainers,
            abuse_contact: rec.abuseContact,
            description: rec.description,
          }
        : null,
      items,
      totalItems,
    };
  }

  async lookupPrefix(parsedPrefix, cursorOffset, limit) {
    const isV6 = parsedPrefix.isV6;
    const allocTable = isV6 ? this.allocV6 : this.allocV4;
    const routeTable = isV6 ? this.routeV6 : this.routeV4;

    const allocKey = isV6
      ? (parsedPrefix.start >> 64n) >> BigInt(64 - this.allocV6B)
      : parsedPrefix.start >>> (32 - this.allocV4B);

    let allocation = null;
    const allocSeg = isV6
      ? this.findSeg(allocTable, allocKey, skipAlloc)
      : this.findSeg4(allocTable, allocKey, skipAlloc);
    if (allocSeg) {
      const regId = readUv(this.bytes, allocSeg.pStart, allocSeg.pEnd);
      const ccId = regId >= 0 ? readUv(this.bytes, _uvNext, allocSeg.pEnd) : -1;
      if (ccId >= 0 && (regId !== 0 || ccId !== 0)) {
        const [startIP, endIP] = slotBoundsToStrings(
          isV6,
          allocTable.bits,
          allocSeg.segStart,
          allocSeg.nextStart,
          allocSeg.hasNext
        );
        const reg = this.dictAt(this.registries, regId);
        const cc = this.countryAt(ccId);
        allocation = {
          start_ip: startIP,
          end_ip: endIP,
          version: isV6 ? 6 : 4,
          registry: reg,
          country_code: cc,
          country_raw: cc,
          name: null,
          status: null,
          allocation_date: null,
          created: null,
          last_modified: null,
          source: null,
          maintainers: null,
          organization: null,
          abuse_contact: null,
          description: null,
        };
      }
    }

    // Collect distinct route CIDRs covering [start, end] from the flattened LPM table.
    const routes = [];
    const seen = new Set();
    let matched = 0;
    let scanned = 0;
    let nextCursor = null;

    if (!isV6) {
      const startNum = parsedPrefix.start >>> 0;
      const endNum = parsedPrefix.end >>> 0;
      let b = this.findBlock4(routeTable, startNum);
      if (b < 0) b = 0;
      const shiftMul = 1 << routeTable.shift;
      outer4: for (; b < routeTable.blocks; b++) {
        let curKey = getKeyNum(this.bytes, routeTable.indexOff + b * routeTable.stride, routeTable.kb);
        let off =
          routeTable.dataOff +
          this.view.getUint32(routeTable.indexOff + b * routeTable.stride + routeTable.kb, true);
        const end =
          b + 1 < routeTable.blocks
            ? routeTable.dataOff +
              this.view.getUint32(routeTable.indexOff + (b + 1) * routeTable.stride + routeTable.kb, true)
            : routeTable.dataOff + routeTable.dataLen;

        while (off < end) {
          if (++scanned > 20000) return { tooBroad: true };
          const enc = readUv(this.bytes, off, end);
          if (enc < 0) break outer4;
          const pStart = _uvNext;
          const delta =
            routeTable.shift === 0
              ? enc
              : enc % 2 === 0
                ? Math.floor(enc / 2) * shiftMul
                : Math.floor(enc / 2);
          curKey += delta;
          const nextOff = skipRoute(this.bytes, pStart, end);
          if (nextOff < 0 || curKey > endNum) break outer4;

          const bits = this.bytes[pStart];
          if (bits > 0) {
            const rVal = readUv(this.bytes, pStart + 1, nextOff);
            const rawRouteVal = rVal >= 0 ? this.resolveRouteVal(rVal) : 0;
            if (rawRouteVal !== 0) {
              const masked = maskIPv4Range(curKey >>> 0, bits);
              if (masked.end >= startNum && masked.start <= endNum) {
                const resolved = this.resolveASNSet(rawRouteVal, false);
                const asns = this.autnumChunks ? await resolved : resolved;
                for (const a of asns) {
                  const key = `${masked.cidr}|${a.asNumber}`;
                  if (seen.has(key)) continue;
                  seen.add(key);
                  if (matched >= cursorOffset) {
                    if (routes.length >= limit) {
                      nextCursor = String(matched);
                      break outer4;
                    }
                    const relation =
                      bits === parsedPrefix.bits
                        ? "exact"
                        : bits < parsedPrefix.bits
                          ? "covering"
                          : "more_specific";
                    routes.push({
                      prefix: masked.cidr,
                      version: 4,
                      origin_asn: `AS${a.asNumber}`,
                      as_number: a.asNumber,
                      relation,
                      registry: a.hasAutnum ? a.registry : null,
                      source: a.hasAutnum ? a.source : null,
                      maintainers: null,
                      organization: a.hasAutnum ? a.organization : null,
                      abuse_contact: null,
                      description: null,
                    });
                  }
                  matched++;
                }
              }
            }
          }
          off = nextOff;
        }
      }
      if (!allocation && routes.length === 0) return null;
      return { allocation, routes, nextCursor };
    }

    const startKey = parsedPrefix.start >> 64n;
    const endKey = parsedPrefix.end >> 64n;

    let b = this.findBlock(routeTable, startKey);
    if (b < 0) b = 0;
    outer: for (; b < routeTable.blocks; b++) {
      let curKey = getKeyBig(this.bytes, routeTable.indexOff + b * routeTable.stride, routeTable.kb);
      let off = routeTable.dataOff + this.view.getUint32(routeTable.indexOff + b * routeTable.stride + routeTable.kb, true);
      const end =
        b + 1 < routeTable.blocks
          ? routeTable.dataOff + this.view.getUint32(routeTable.indexOff + (b + 1) * routeTable.stride + routeTable.kb, true)
          : routeTable.dataOff + routeTable.dataLen;

      while (off < end) {
        if (++scanned > 20000) {
          return { tooBroad: true };
        }
        const d = readUvarintBig(this.bytes, off, end);
        if (!d) break outer;
        curKey += decodeDelta(routeTable, d[0]);
        const pStart = d[1];
        const nextOff = skipRoute(this.bytes, pStart, end);
        if (nextOff < 0) break outer;
        if (curKey > endKey) break outer;

        // Check if this segment reaches >= startKey (we also include the segment right before startKey if it covers startKey).
        const bits = this.bytes[pStart];
        if (bits > 0) {
          const rVal = readUv(this.bytes, pStart + 1, nextOff);
          const rawRouteVal = rVal >= 0 ? this.resolveRouteVal(rVal) : 0;
          if (rawRouteVal !== 0) {
            const masked = maskIPv6Range(curKey << 64n, bits);
            // Only emit if masked range overlaps [parsedPrefix.start, parsedPrefix.end].
            if (masked.end >= parsedPrefix.start && masked.start <= parsedPrefix.end) {
              const resolved = this.resolveASNSet(rawRouteVal, false);
              const asns = this.autnumChunks ? await resolved : resolved;
              for (const a of asns) {
                const key = `${masked.cidr}|${a.asNumber}`;
                if (seen.has(key)) continue;
                seen.add(key);
                if (matched >= cursorOffset) {
                  if (routes.length >= limit) {
                    nextCursor = String(matched);
                    break outer;
                  }
                  const relation =
                    bits === parsedPrefix.bits
                      ? "exact"
                      : bits < parsedPrefix.bits
                        ? "covering"
                        : "more_specific";
                  routes.push({
                    prefix: masked.cidr,
                    version: 6,
                    origin_asn: `AS${a.asNumber}`,
                    as_number: a.asNumber,
                    relation,
                    registry: a.hasAutnum ? a.registry : null,
                    source: a.hasAutnum ? a.source : null,
                    maintainers: null,
                    organization: a.hasAutnum ? a.organization : null,
                    abuse_contact: null,
                    description: null,
                  });
                }
                matched++;
              }
            }
          }
        }
        off = nextOff;
      }
    }

    if (!allocation && routes.length === 0) return null;
    return { allocation, routes, nextCursor };
  }
}

function skipVarint(bytes, pos, end) {
  for (let i = 0; pos < end && i < 10; i++) {
    if (bytes[pos++] < 0x80) return pos;
  }
  return -1;
}

function skipAlloc(bytes, pos, end) {
  const p = skipVarint(bytes, pos, end);
  return p < 0 ? -1 : skipVarint(bytes, p, end);
}

function skipGeo(bytes, pos, end) {
  return skipVarint(bytes, pos, end);
}

function skipRoute(bytes, pos, end) {
  if (pos >= end) return -1;
  return bytes[pos++] === 0 ? pos : skipVarint(bytes, pos, end);
}

function slotBoundsToStrings(isV6, bits, startSlot, nextSlot, hasNext) {
  if (!isV6) {
    const shift = 32 - bits;
    const s = shift >= 32 ? 0 : (startSlot << shift) >>> 0;
    const e = hasNext ? (((shift >= 32 ? 0 : nextSlot << shift) - 1) >>> 0) : 0xffffffff;
    return [formatIPv4(s), formatIPv4(e)];
  }
  const shift = BigInt(64 - bits);
  const s = (startSlot << shift) << 64n;
  const eHi = hasNext ? (nextSlot << shift) - 1n : 0xffffffffffffffffn;
  const e = (eHi << 64n) | 0xffffffffffffffffn;
  return [formatIPv6(s), formatIPv6(e)];
}
