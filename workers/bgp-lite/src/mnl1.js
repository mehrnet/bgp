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

function readStringDict(bytes, pos) {
  const c = readUvarint(bytes, pos, bytes.length);
  if (!c) throw new Error("bad dict count");
  const count = c[0];
  pos = c[1];
  const out = new Array(count);
  for (let i = 0; i < count; i++) {
    const l = readUvarint(bytes, pos, bytes.length);
    if (!l) throw new Error("bad string len");
    const len = l[0];
    pos = l[1];
    out[i] = decoder.decode(bytes.subarray(pos, pos + len));
    pos += len;
  }
  return [out, pos];
}

function openSegTable(bytes, view, pos, bits, blockSize) {
  const entries = view.getUint32(pos, true);
  const dataLen = view.getUint32(pos + 4, true);
  pos += 8;
  const kb = (bits + 7) >> 3;
  const blocks = Math.ceil(entries / blockSize);
  const indexLen = blocks * (kb + 4);
  return [
    { bits, kb, stride: kb + 4, entries, blocks, indexOff: pos, dataOff: pos + indexLen, dataLen },
    pos + indexLen + dataLen,
  ];
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
    if (
      bytes.length < 13 ||
      bytes[0] !== 0x4d || // M
      bytes[1] !== 0x4e || // N
      bytes[2] !== 0x4c || // L
      bytes[3] !== 0x31 || // 1
      bytes[4] !== 2
    ) {
      throw new Error("invalid MNL1 header");
    }
    this.bytes = bytes;
    this.view = view;
    this.flags = bytes[5];
    this.blockSize = bytes[6];
    this.allocV4B = bytes[7];
    this.allocV6B = bytes[8];
    this.routeV4B = bytes[9];
    this.routeV6B = bytes[10];
    const metaLen = view.getUint16(11, true);
    let pos = 13;
    this.meta = metaLen > 0 ? JSON.parse(decoder.decode(bytes.subarray(pos, pos + metaLen))) : {};
    pos += metaLen;

    const ccCount = view.getUint16(pos, true);
    pos += 2;
    this.countriesOff = pos;
    this.ccCount = ccCount;
    pos += 2 * ccCount;

    [this.registries, pos] = readStringDict(bytes, pos);
    [this.statuses, pos] = readStringDict(bytes, pos);
    [this.sources, pos] = readStringDict(bytes, pos);
    [this.regions, pos] = readStringDict(bytes, pos);
    [this.cities, pos] = readStringDict(bytes, pos);

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
    this.asnOffsetsOff = pos;
    pos += (this.asnCount + 1) * 4;
    this.asnDataOff = pos;
    pos += view.getUint32(this.asnOffsetsOff + this.asnCount * 4, true);

    this.multiCount = view.getUint32(pos, true);
    pos += 4;
    this.multiOffsetsOff = pos;
    pos += (this.multiCount + 1) * 4;
    this.multiDataOff = pos;
    pos += view.getUint32(this.multiOffsetsOff + this.multiCount * 4, true);

    [this.allocV4, pos] = openSegTable(bytes, view, pos, this.allocV4B, this.blockSize);
    [this.allocV6, pos] = openSegTable(bytes, view, pos, this.allocV6B, this.blockSize);
    [this.geoV4, pos] = openSegTable(bytes, view, pos, this.allocV4B, this.blockSize);
    [this.geoV6, pos] = openSegTable(bytes, view, pos, this.allocV6B, this.blockSize);
    [this.routeV4, pos] = openSegTable(bytes, view, pos, this.routeV4B, this.blockSize);
    [this.routeV6, pos] = openSegTable(bytes, view, pos, this.routeV6B, this.blockSize);

    if ((this.flags & 1) !== 0) {
      this.asnRouteOffsetsOff = pos;
      pos += (this.asnCount + 1) * 4;
      this.asnRouteDataOff = pos;
    } else {
      this.asnRouteOffsetsOff = -1;
      this.asnRouteDataOff = -1;
    }
  }

  countryAt(idx) {
    if (idx <= 0 || idx > this.ccCount) return null;
    const p = this.countriesOff + (idx - 1) * 2;
    return String.fromCharCode(this.bytes[p], this.bytes[p + 1]);
  }

  dictAt(list, idx) {
    if (idx <= 0 || idx > list.length) return null;
    return list[idx - 1];
  }

  autnumByIdx(idx) {
    if (idx < 1 || idx > this.asnCount) return { asNumber: 0, hasAutnum: false };
    const asn = this.view.getUint32(this.asnNumbersOff + (idx - 1) * 4, true);
    const off = this.asnDataOff + this.view.getUint32(this.asnOffsetsOff + (idx - 1) * 4, true);
    const end = this.asnDataOff + this.view.getUint32(this.asnOffsetsOff + idx * 4, true);
    if (off >= end || this.bytes[off] === 0) {
      return { asNumber: asn, hasAutnum: false };
    }
    let p = off + 1;
    const r1 = readUvarint(this.bytes, p, end);
    const r2 = readUvarint(this.bytes, r1[1], end);
    const r3 = readUvarint(this.bytes, r2[1], end);
    const r4 = readUvarint(this.bytes, r3[1], end);
    const r5 = readUvarint(this.bytes, r4[1], end);
    p = r5[1];
    const name = r5[0] > 0 ? decoder.decode(this.bytes.subarray(p, p + r5[0])) : null;
    p += r5[0];
    const r6 = readUvarint(this.bytes, p, end);
    p = r6[1];
    const org = r6[0] > 0 ? decoder.decode(this.bytes.subarray(p, p + r6[0])) : null;
    return {
      asNumber: asn,
      hasAutnum: true,
      registry: this.dictAt(this.registries, r1[0]),
      country: this.countryAt(r2[0]),
      status: this.dictAt(this.statuses, r3[0]),
      source: this.dictAt(this.sources, r4[0]),
      name,
      organization: org,
    };
  }

  resolveASNSet(val) {
    if (val === 0) return [];
    if (val <= this.asnCount) {
      return [this.autnumByIdx(val)];
    }
    const mIdx = val - this.asnCount - 1;
    if (mIdx < 0 || mIdx >= this.multiCount) return [];
    const off = this.multiDataOff + this.view.getUint32(this.multiOffsetsOff + mIdx * 4, true);
    const end = this.multiDataOff + this.view.getUint32(this.multiOffsetsOff + (mIdx + 1) * 4, true);
    const c = readUvarint(this.bytes, off, end);
    if (!c) return [];
    const count = c[0];
    let p = c[1];
    const out = new Array(count);
    for (let i = 0; i < count; i++) {
      const r = readUvarint(this.bytes, p, end);
      out[i] = this.autnumByIdx(r[0]);
      p = r[1];
    }
    return out;
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
      curKey += d[0];
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

  lookupIP(parsedIP) {
    const isV6 = parsedIP.isV6;
    const allocTable = isV6 ? this.allocV6 : this.allocV4;
    const geoTable = isV6 ? this.geoV6 : this.geoV4;
    const routeTable = isV6 ? this.routeV6 : this.routeV4;

    const allocKey = isV6
      ? (parsedIP.value >> 64n) >> BigInt(64 - this.allocV6B)
      : BigInt(parsedIP.value >>> (32 - this.allocV4B));
    const routeKey = isV6
      ? (parsedIP.value >> 64n) >> BigInt(64 - this.routeV6B)
      : BigInt(parsedIP.value >>> 0);

    let hasAlloc = false;
    let registry = null;
    let allocCC = null;
    let allocStart = null;
    let allocEnd = null;
    const allocSeg = this.findSeg(allocTable, allocKey, skipAlloc);
    if (allocSeg) {
      const r1 = readUvarint(this.bytes, allocSeg.pStart, allocSeg.pEnd);
      const r2 = readUvarint(this.bytes, r1[1], allocSeg.pEnd);
      if (r1[0] !== 0 || r2[0] !== 0) {
        hasAlloc = true;
        registry = this.dictAt(this.registries, r1[0]);
        allocCC = this.countryAt(r2[0]);
        [allocStart, allocEnd] = slotBoundsToStrings(isV6, allocTable.bits, allocSeg.segStart, allocSeg.nextStart, allocSeg.hasNext);
      }
    }

    let hasGeo = false;
    let geoCC = null;
    let geoRegion = null;
    let geoCity = null;
    const geoSeg = this.findSeg(geoTable, allocKey, skipGeo);
    if (geoSeg) {
      const r1 = readUvarint(this.bytes, geoSeg.pStart, geoSeg.pEnd);
      const locID = r1 ? r1[0] : 0;
      if (locID > 0 && locID <= this.geoLocCount) {
        const idx = locID - 1;
        const off = this.geoLocDataOff + this.view.getUint32(this.geoLocOffsetsOff + idx * 4, true);
        const end = this.geoLocDataOff + this.view.getUint32(this.geoLocOffsetsOff + (idx + 1) * 4, true);
        const a = readUvarint(this.bytes, off, end);
        const b = a && readUvarint(this.bytes, a[1], end);
        const c = b && readUvarint(this.bytes, b[1], end);
        if (c) {
          hasGeo = true;
          geoCC = this.countryAt(a[0]);
          geoRegion = this.dictAt(this.regions, b[0]);
          geoCity = this.dictAt(this.cities, c[0]);
        }
      }
    }

    let hasRoute = false;
    let routeNet = null;
    let routeASNs = [];
    const routeSeg = this.findSeg(routeTable, routeKey, skipRoute);
    if (routeSeg && routeSeg.pStart < routeSeg.pEnd) {
      const bits = this.bytes[routeSeg.pStart];
      if (bits > 0) {
        const r1 = readUvarint(this.bytes, routeSeg.pStart + 1, routeSeg.pEnd);
        if (r1 && r1[0] !== 0) {
          hasRoute = true;
          if (isV6) {
            const { start, end, cidr } = maskIPv6Range(parsedIP.value, bits);
            routeNet = { cidr, startIP: formatIPv6(start), endIP: formatIPv6(end) };
          } else {
            const { start, end, cidr } = maskIPv4Range(parsedIP.value, bits);
            routeNet = { cidr, startIP: formatIPv4(start), endIP: formatIPv4(end) };
          }
          routeASNs = this.resolveASNSet(r1[0]);
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

  lookupASN(asNumber, offset, limit) {
    const idx = this.findASNIndex(asNumber);
    if (idx < 0) return null;
    const rec = this.autnumByIdx(idx + 1);
    const items = [];
    let totalItems = 0;

    if (this.asnRouteOffsetsOff >= 0) {
      let p = this.asnRouteDataOff + this.view.getUint32(this.asnRouteOffsetsOff + idx * 4, true);
      const end = this.asnRouteDataOff + this.view.getUint32(this.asnRouteOffsetsOff + (idx + 1) * 4, true);
      const asnStr = `AS${asNumber}`;
      while (p < end) {
        const rawLen = this.bytes[p++];
        const isV6 = rawLen >= 128;
        const bits = isV6 ? rawLen - 128 : rawLen;
        const nb = (bits + 7) >> 3;
        if (p + nb > end) break;
        if (totalItems >= offset && items.length < limit) {
          let prefix;
          if (isV6) {
            let v = 0n;
            for (let i = 0; i < nb; i++) {
              v |= BigInt(this.bytes[p + i]) << BigInt(8 * (15 - i));
            }
            prefix = `${formatIPv6(v)}/${bits}`;
          } else {
            let v = 0;
            for (let i = 0; i < nb; i++) {
              v |= this.bytes[p + i] << (8 * (3 - i));
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
            maintainers: null,
            abuse_contact: null,
            description: null,
          }
        : null,
      items,
      totalItems,
    };
  }

  lookupPrefix(parsedPrefix, cursorOffset, limit) {
    const isV6 = parsedPrefix.isV6;
    const allocTable = isV6 ? this.allocV6 : this.allocV4;
    const routeTable = isV6 ? this.routeV6 : this.routeV4;

    const allocKey = isV6
      ? (parsedPrefix.start >> 64n) >> BigInt(64 - this.allocV6B)
      : BigInt(parsedPrefix.start >>> (32 - this.allocV4B));

    let allocation = null;
    const allocSeg = this.findSeg(allocTable, allocKey, skipAlloc);
    if (allocSeg) {
      const r1 = readUvarint(this.bytes, allocSeg.pStart, allocSeg.pEnd);
      const r2 = readUvarint(this.bytes, r1[1], allocSeg.pEnd);
      if (r1[0] !== 0 || r2[0] !== 0) {
        const [startIP, endIP] = slotBoundsToStrings(
          isV6,
          allocTable.bits,
          allocSeg.segStart,
          allocSeg.nextStart,
          allocSeg.hasNext
        );
        const reg = this.dictAt(this.registries, r1[0]);
        const cc = this.countryAt(r2[0]);
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
    const startKey = isV6 ? parsedPrefix.start >> 64n : BigInt(parsedPrefix.start >>> 0);
    const endKey = isV6 ? parsedPrefix.end >> 64n : BigInt(parsedPrefix.end >>> 0);

    const routes = [];
    const seen = new Set();
    let matched = 0;
    let scanned = 0;
    let nextCursor = null;

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
        curKey += d[0];
        const pStart = d[1];
        const nextOff = skipRoute(this.bytes, pStart, end);
        if (nextOff < 0) break outer;
        if (curKey > endKey) break outer;

        // Check if this segment reaches >= startKey (we also include the segment right before startKey if it covers startKey).
        const bits = this.bytes[pStart];
        if (bits > 0) {
          const r1 = readUvarint(this.bytes, pStart + 1, nextOff);
          if (r1 && r1[0] !== 0) {
            const addrVal = isV6 ? curKey << 64n : Number(curKey) >>> 0;
            const masked = isV6 ? maskIPv6Range(addrVal, bits) : maskIPv4Range(addrVal, bits);
            // Only emit if masked range overlaps [parsedPrefix.start, parsedPrefix.end].
            if (masked.end >= parsedPrefix.start && masked.start <= parsedPrefix.end) {
              const asns = this.resolveASNSet(r1[0]);
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
                    version: isV6 ? 6 : 4,
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

function skipAlloc(bytes, pos, end) {
  const r1 = readUvarint(bytes, pos, end);
  if (!r1) return -1;
  const r2 = readUvarint(bytes, r1[1], end);
  return r2 ? r2[1] : -1;
}

function skipGeo(bytes, pos, end) {
  const r1 = readUvarint(bytes, pos, end);
  return r1 ? r1[1] : -1;
}

function skipRoute(bytes, pos, end) {
  if (pos >= end) return -1;
  const bits = bytes[pos++];
  if (bits === 0) return pos;
  const r1 = readUvarint(bytes, pos, end);
  return r1 ? r1[1] : -1;
}

function slotBoundsToStrings(isV6, bits, startSlot, nextSlot, hasNext) {
  if (!isV6) {
    const shift = BigInt(32 - bits);
    const s = Number((startSlot << shift) & 0xffffffffn) >>> 0;
    const e = hasNext ? Number(((nextSlot << shift) - 1n) & 0xffffffffn) >>> 0 : 0xffffffff >>> 0;
    return [formatIPv4(s), formatIPv4(e)];
  }
  const shift = BigInt(64 - bits);
  const s = (startSlot << shift) << 64n;
  const eHi = hasNext ? (nextSlot << shift) - 1n : 0xffffffffffffffffn;
  const e = (eHi << 64n) | 0xffffffffffffffffn;
  return [formatIPv6(s), formatIPv6(e)];
}
