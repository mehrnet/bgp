import {
  UvCursor,
  inflateRaw,
  openChunkedSection,
  openSegTable,
  readLazyDict,
  readOffsetDict,
  skipAlloc,
  skipGeo,
  skipRoute,
  skipVarint,
  slotBoundsToStrings,
} from "./binary.ts";
import { formatIPv4, formatIPv6, maskIPv4Range, maskIPv6Range } from "./ip.ts";
import { scanRoutesV4, scanRoutesV6 } from "./prefixScan.ts";
import { findSeg, findSeg4 } from "./seg.ts";
import type {
  AllocationItem,
  AsnLookupResult,
  AutnumRecord,
  ChunkedSection,
  DatasetMeta,
  Dict,
  IpLookupResult,
  ParsedIP,
  ParsedPrefix,
  PrefixLookupResult,
  RouteItem,
  SegTable,
} from "./types.ts";

const decoder = new TextDecoder();
const uv = new UvCursor();

export class LiteDB {
  readonly version: number;
  readonly bytes: Uint8Array;
  readonly view: DataView;
  readonly flags: number;
  readonly blockSize: number;
  readonly allocV4B: number;
  readonly allocV6B: number;
  readonly routeV4B: number;
  readonly routeV6B: number;
  readonly geoV4B: number;
  readonly geoV6B: number;
  readonly meta: DatasetMeta;
  readonly countriesOff: number;
  readonly ccCount: number;
  readonly registries: Dict;
  readonly statuses: Dict;
  readonly sources: Dict;
  readonly regions: Dict;
  readonly cities: Dict;
  readonly geoLocCount: number;
  readonly geoLocOffsetsOff: number;
  readonly geoLocDataOff: number;
  readonly asnCount: number;
  readonly asnNumbersOff: number;
  readonly autnumChunks: ChunkedSection | null;
  readonly asnOffsetsOff: number;
  readonly asnDataOff: number;
  readonly multiCount: number;
  readonly multiOffsetsOff: number;
  readonly multiDataOff: number;
  readonly routeValCount: number;
  readonly routeValOff: number;
  readonly allocV4: SegTable;
  readonly allocV6: SegTable;
  readonly geoV4: SegTable;
  readonly geoV6: SegTable;
  readonly routeV4: SegTable;
  readonly routeV6: SegTable;
  readonly asnRouteChunks: ChunkedSection | null;
  readonly asnRouteOffsetsOff: number;
  readonly asnRouteDataOff: number;

  constructor(buffer: ArrayBufferLike) {
    const bytes = new Uint8Array(buffer);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const ver = bytes[4] ?? 0;
    if (
      bytes.length < 13 ||
      bytes[0] !== 0x4d ||
      bytes[1] !== 0x4e ||
      bytes[2] !== 0x4c ||
      bytes[3] !== 0x31 ||
      ver < 3 ||
      ver > 7
    ) {
      throw new Error("invalid MNL1 header");
    }
    this.version = ver;
    this.bytes = bytes;
    this.view = view;
    this.flags = bytes[5] ?? 0;
    this.blockSize = bytes[6] ?? 0;
    this.allocV4B = bytes[7] ?? 0;
    this.allocV6B = bytes[8] ?? 0;
    this.routeV4B = bytes[9] ?? 0;
    this.routeV6B = bytes[10] ?? 0;

    let pos = 13;
    if (ver >= 4) {
      if (bytes.length < 15) throw new Error("invalid MNL1 header");
      this.geoV4B = bytes[11] ?? 0;
      this.geoV6B = bytes[12] ?? 0;
      pos = 15;
    } else {
      this.geoV4B = this.allocV4B;
      this.geoV6B = this.allocV6B;
    }

    const metaLen = view.getUint16(pos - 2, true);
    this.meta = metaLen > 0 ? (JSON.parse(decoder.decode(bytes.subarray(pos, pos + metaLen))) as DatasetMeta) : {};
    pos += metaLen;

    this.ccCount = view.getUint16(pos, true);
    pos += 2;
    this.countriesOff = pos;
    pos += 2 * this.ccCount;

    const readDict = (p: number): [Dict, number] => (ver >= 6 ? readOffsetDict(view, p) : readLazyDict(bytes, p));
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
    [this.allocV4, pos] = openSegTable(view, pos, this.allocV4B, this.blockSize, false, hasShifted);
    [this.allocV6, pos] = openSegTable(view, pos, this.allocV6B, this.blockSize, true, hasShifted);
    [this.geoV4, pos] = openSegTable(view, pos, this.geoV4B, this.blockSize, false, hasShifted);
    [this.geoV6, pos] = openSegTable(view, pos, this.geoV6B, this.blockSize, true, hasShifted);
    [this.routeV4, pos] = openSegTable(view, pos, this.routeV4B, this.blockSize, false, hasShifted);
    [this.routeV6, pos] = openSegTable(view, pos, this.routeV6B, this.blockSize, true, hasShifted);

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

  async readChunkEntry(
    section: ChunkedSection | null,
    legacyOffsetsOff: number,
    legacyDataOff: number,
    zeroBasedIdx: number
  ): Promise<{ bytes: Uint8Array; off: number; end: number }> {
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
        const firstKey = section.cache.keys().next().value;
        if (firstKey !== undefined) section.cache.delete(firstKey);
      }
      section.cache.set(chunkIdx, chunk);
    }
    const off = chunk.view.getUint32(localIdx * 4, true);
    const end = chunk.view.getUint32((localIdx + 1) * 4, true);
    return { bytes: chunk.bytes, off, end };
  }

  countryAt(idx: number): string | null {
    if (idx <= 0 || idx > this.ccCount) return null;
    const p = this.countriesOff + (idx - 1) * 2;
    return String.fromCharCode(this.bytes[p] ?? 0, this.bytes[p + 1] ?? 0);
  }

  dictAt(dict: Dict, idx: number): string | null {
    if (idx <= 0) return null;
    if (dict.isOffset) {
      if (idx > dict.count) return null;
      const off = dict.dataOff + this.view.getUint32(dict.offsetsOff + (idx - 1) * 4, true);
      const end = dict.dataOff + this.view.getUint32(dict.offsetsOff + idx * 4, true);
      return decoder.decode(this.bytes.subarray(off, end));
    }
    if (idx > dict.lens.length) return null;
    const off = dict.offs[idx - 1] ?? 0;
    const len = dict.lens[idx - 1] ?? 0;
    return decoder.decode(this.bytes.subarray(off, off + len));
  }

  resolveRouteVal(val: number): number {
    if (val === 0 || this.routeValCount === 0) return val;
    if (val > this.routeValCount) return 0;
    return this.view.getUint32(this.routeValOff + (val - 1) * 4, true);
  }

  decodeAutnumFrom(bytes: Uint8Array, off: number, end: number, asn: number, summaryOnly: boolean): AutnumRecord {
    if (off >= end || (bytes[off] ?? 0) === 0) return { asNumber: asn, hasAutnum: false };
    let p = off + 1;
    if (summaryOnly) {
      for (let i = 0; i < 4; i++) {
        p = skipVarint(bytes, p, end);
        if (p < 0) return { asNumber: asn, hasAutnum: false };
      }
      const nameLen = uv.read(bytes, p, end);
      if (nameLen < 0) return { asNumber: asn, hasAutnum: false };
      p = uv.next;
      const name = nameLen > 0 ? decoder.decode(bytes.subarray(p, p + nameLen)) : null;
      p += nameLen;
      const orgLen = uv.read(bytes, p, end);
      if (orgLen < 0) return { asNumber: asn, hasAutnum: true, name, organization: null };
      p = uv.next;
      const org = orgLen > 0 ? decoder.decode(bytes.subarray(p, p + orgLen)) : null;
      return { asNumber: asn, hasAutnum: true, name, organization: org };
    }
    const r1 = uv.read(bytes, p, end);
    const r2 = uv.read(bytes, uv.next, end);
    const r3 = uv.read(bytes, uv.next, end);
    const r4 = uv.read(bytes, uv.next, end);
    const nameLen = uv.read(bytes, uv.next, end);
    p = uv.next;
    const name = nameLen > 0 ? decoder.decode(bytes.subarray(p, p + nameLen)) : null;
    p += nameLen;
    const orgLen = uv.read(bytes, p, end);
    p = uv.next;
    const org = orgLen > 0 ? decoder.decode(bytes.subarray(p, p + orgLen)) : null;
    p += orgLen;
    const mntLen = uv.read(bytes, p, end);
    p = uv.next;
    const maintainers = mntLen > 0 ? decoder.decode(bytes.subarray(p, p + mntLen)) : null;
    p += mntLen;
    const acLen = uv.read(bytes, p, end);
    p = uv.next;
    const abuseContact = acLen > 0 ? decoder.decode(bytes.subarray(p, p + acLen)) : null;
    p += acLen;
    const descLen = uv.read(bytes, p, end);
    p = uv.next;
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

  autnumByIdxSync(idx: number, summaryOnly = false): AutnumRecord {
    if (idx < 1 || idx > this.asnCount) return { asNumber: 0, hasAutnum: false };
    const asn = this.view.getUint32(this.asnNumbersOff + (idx - 1) * 4, true);
    const off = this.asnDataOff + this.view.getUint32(this.asnOffsetsOff + (idx - 1) * 4, true);
    const end = this.asnDataOff + this.view.getUint32(this.asnOffsetsOff + idx * 4, true);
    return this.decodeAutnumFrom(this.bytes, off, end, asn, summaryOnly);
  }

  async autnumByIdx(idx: number, summaryOnly = false): Promise<AutnumRecord> {
    if (!this.autnumChunks) return this.autnumByIdxSync(idx, summaryOnly);
    if (idx < 1 || idx > this.asnCount) return { asNumber: 0, hasAutnum: false };
    const asn = this.view.getUint32(this.asnNumbersOff + (idx - 1) * 4, true);
    const { bytes, off, end } = await this.readChunkEntry(this.autnumChunks, this.asnOffsetsOff, this.asnDataOff, idx - 1);
    return this.decodeAutnumFrom(bytes, off, end, asn, summaryOnly);
  }

  resolveASNIndices(val: number): number[] {
    if (val === 0) return [];
    if (val <= this.asnCount) return [val];
    const mIdx = val - this.asnCount - 1;
    if (mIdx < 0 || mIdx >= this.multiCount) return [];
    const off = this.multiDataOff + this.view.getUint32(this.multiOffsetsOff + mIdx * 4, true);
    const end = this.multiDataOff + this.view.getUint32(this.multiOffsetsOff + (mIdx + 1) * 4, true);
    const count = uv.read(this.bytes, off, end);
    if (count < 0) return [];
    let p = uv.next;
    const indices = new Array<number>(count);
    for (let i = 0; i < count; i++) {
      const v = uv.read(this.bytes, p, end);
      if (v < 0) return [];
      indices[i] = v;
      p = uv.next;
    }
    return indices;
  }

  resolveASNSet(val: number, summaryOnly = false): AutnumRecord[] | Promise<AutnumRecord[]> {
    const indices = this.resolveASNIndices(val);
    if (indices.length === 0) return [];
    if (!this.autnumChunks) return indices.map((i) => this.autnumByIdxSync(i, summaryOnly));
    return Promise.all(indices.map((i) => this.autnumByIdx(i, summaryOnly)));
  }

  lookupCountry(parsedIP: ParsedIP): string | null {
    const geoSeg = parsedIP.isV6
      ? findSeg(this.bytes, this.view, this.geoV6, (parsedIP.value >> 64n) >> BigInt(64 - this.geoV6B), skipGeo)
      : findSeg4(this.bytes, this.view, this.geoV4, parsedIP.value >>> (32 - this.geoV4B), skipGeo);
    if (geoSeg) {
      const locID = uv.read(this.bytes, geoSeg.pStart, geoSeg.pEnd);
      if (locID > 0 && locID <= this.geoLocCount) {
        const idx = locID - 1;
        const off = this.geoLocDataOff + this.view.getUint32(this.geoLocOffsetsOff + idx * 4, true);
        const end = this.geoLocDataOff + this.view.getUint32(this.geoLocOffsetsOff + (idx + 1) * 4, true);
        const ccRaw = uv.read(this.bytes, off, end);
        if (ccRaw >= 0) {
          const cc = this.countryAt(this.version >= 7 ? ccRaw >>> 1 : ccRaw);
          if (cc) return cc;
        }
      }
    }
    const allocSeg = parsedIP.isV6
      ? findSeg(this.bytes, this.view, this.allocV6, (parsedIP.value >> 64n) >> BigInt(64 - this.allocV6B), skipAlloc)
      : findSeg4(this.bytes, this.view, this.allocV4, parsedIP.value >>> (32 - this.allocV4B), skipAlloc);
    if (allocSeg) {
      const regId = uv.read(this.bytes, allocSeg.pStart, allocSeg.pEnd);
      if (regId >= 0) {
        const ccId = uv.read(this.bytes, uv.next, allocSeg.pEnd);
        if (ccId >= 0) return this.countryAt(ccId);
      }
    }
    return null;
  }

  async lookupIP(parsedIP: ParsedIP): Promise<IpLookupResult | null> {
    const isV6 = parsedIP.isV6;
    const allocTable = isV6 ? this.allocV6 : this.allocV4;
    const geoTable = isV6 ? this.geoV6 : this.geoV4;
    const routeTable = isV6 ? this.routeV6 : this.routeV4;

    let hasAlloc = false;
    let registry: string | null = null;
    let allocCC: string | null = null;
    let allocStart: string | null = null;
    let allocEnd: string | null = null;
    const allocSeg = isV6
      ? findSeg(this.bytes, this.view, allocTable, (parsedIP.value >> 64n) >> BigInt(64 - this.allocV6B), skipAlloc)
      : findSeg4(this.bytes, this.view, allocTable, parsedIP.value >>> (32 - this.allocV4B), skipAlloc);
    if (allocSeg) {
      const regId = uv.read(this.bytes, allocSeg.pStart, allocSeg.pEnd);
      const ccId = regId >= 0 ? uv.read(this.bytes, uv.next, allocSeg.pEnd) : -1;
      if (ccId >= 0 && (regId !== 0 || ccId !== 0)) {
        hasAlloc = true;
        registry = this.dictAt(this.registries, regId);
        allocCC = this.countryAt(ccId);
        [allocStart, allocEnd] = slotBoundsToStrings(isV6, allocTable.bits, allocSeg.segStart, allocSeg.nextStart, allocSeg.hasNext);
      }
    }

    let hasGeo = false;
    let geoCC: string | null = null;
    let geoRegion: string | null = null;
    let geoCity: string | null = null;
    let geoLat: number | null = null;
    let geoLon: number | null = null;
    const geoSeg = isV6
      ? findSeg(this.bytes, this.view, geoTable, (parsedIP.value >> 64n) >> BigInt(64 - this.geoV6B), skipGeo)
      : findSeg4(this.bytes, this.view, geoTable, parsedIP.value >>> (32 - this.geoV4B), skipGeo);
    if (geoSeg) {
      const locID = uv.read(this.bytes, geoSeg.pStart, geoSeg.pEnd);
      if (locID > 0 && locID <= this.geoLocCount) {
        const idx = locID - 1;
        const off = this.geoLocDataOff + this.view.getUint32(this.geoLocOffsetsOff + idx * 4, true);
        const end = this.geoLocDataOff + this.view.getUint32(this.geoLocOffsetsOff + (idx + 1) * 4, true);
        const ccRaw = uv.read(this.bytes, off, end);
        const ccId = this.version >= 7 ? ccRaw >>> 1 : ccRaw;
        const hasCoords = this.version >= 7 && ccRaw >= 0 && (ccRaw & 1) !== 0;
        const regId = ccRaw >= 0 ? uv.read(this.bytes, uv.next, end) : -1;
        const cityId = regId >= 0 ? uv.read(this.bytes, uv.next, end) : -1;
        if (cityId >= 0) {
          hasGeo = true;
          geoCC = this.countryAt(ccId);
          geoRegion = this.dictAt(this.regions, regId);
          geoCity = this.dictAt(this.cities, cityId);
          if (hasCoords && uv.next + 8 <= end) {
            geoLat = this.view.getInt32(uv.next, true) / 1_000_000;
            geoLon = this.view.getInt32(uv.next + 4, true) / 1_000_000;
          }
        }
      }
    }

    let hasRoute = false;
    let routeNet: { cidr: string; startIP: string; endIP: string } | null = null;
    let routeASNs: AutnumRecord[] = [];
    const routeSeg = isV6
      ? findSeg(this.bytes, this.view, routeTable, (parsedIP.value >> 64n) >> BigInt(64 - this.routeV6B), skipRoute)
      : findSeg4(this.bytes, this.view, routeTable, parsedIP.value >>> 0, skipRoute);
    if (routeSeg && routeSeg.pStart < routeSeg.pEnd) {
      const bits = this.bytes[routeSeg.pStart] ?? 0;
      if (bits > 0) {
        const rVal = uv.read(this.bytes, routeSeg.pStart + 1, routeSeg.pEnd);
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
          routeASNs = this.autnumChunks ? await resolved : (resolved as AutnumRecord[]);
        }
      }
    }

    if (!hasAlloc && !hasGeo && !hasRoute) return null;
    const asnStrings = routeASNs.map((a) => `AS${a.asNumber}`);
    const singleASN = asnStrings.length === 1;
    const firstAsn = routeASNs[0];
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
        asn: singleASN ? (asnStrings[0] ?? null) : null,
        asns: asnStrings,
        origins: routeASNs.map((a) => ({
          asn: `AS${a.asNumber}`,
          as_number: a.asNumber,
          name: a.hasAutnum ? (a.name ?? null) : null,
          organization: a.hasAutnum ? (a.organization ?? null) : null,
        })),
        as_number: singleASN && firstAsn ? firstAsn.asNumber : null,
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
      location: { country_code: geoCC || allocCC, region: geoRegion, city: geoCity, latitude: geoLat, longitude: geoLon },
      sources: { allocation: hasAlloc, route: hasRoute, geofeed: hasGeo },
    };
  }

  findASNIndex(asNumber: number): number {
    let lo = 0;
    let hi = this.asnCount;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      const v = this.view.getUint32(this.asnNumbersOff + mid * 4, true);
      if (v < asNumber) lo = mid + 1;
      else hi = mid;
    }
    if (lo >= this.asnCount || this.view.getUint32(this.asnNumbersOff + lo * 4, true) !== asNumber) return -1;
    return lo;
  }

  async lookupASN(asNumber: number, offset: number, limit: number, needTotal = true): Promise<AsnLookupResult | null> {
    const idx = this.findASNIndex(asNumber);
    if (idx < 0) return null;
    const rec = await this.autnumByIdx(idx + 1);
    const items: RouteItem[] = [];
    let totalItems = 0;

    if ((this.flags & 1) !== 0) {
      const entry = await this.readChunkEntry(this.asnRouteChunks, this.asnRouteOffsetsOff, this.asnRouteDataOff, idx);
      const rBytes = entry.bytes;
      let p = entry.off;
      const end = entry.end;
      const asnStr = `AS${asNumber}`;
      const stopAt = offset + limit;
      while (p < end) {
        const rawLen = rBytes[p++] ?? 0;
        const isV6 = rawLen >= 128;
        const bits = isV6 ? rawLen - 128 : rawLen;
        const nb = (bits + 7) >> 3;
        if (p + nb > end) break;
        if (totalItems >= offset && items.length < limit) {
          let prefix: string;
          if (isV6) {
            let v = 0n;
            for (let i = 0; i < nb; i++) v |= BigInt(rBytes[p + i] ?? 0) << BigInt(8 * (15 - i));
            prefix = `${formatIPv6(v)}/${bits}`;
          } else {
            let v = 0;
            for (let i = 0; i < nb; i++) v |= (rBytes[p + i] ?? 0) << (8 * (3 - i));
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
        if (!needTotal && totalItems > stopAt) break;
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
            registry: rec.registry ?? null,
            country_code: rec.country ?? null,
            country_raw: rec.country ?? null,
            name: rec.name ?? null,
            organization: rec.organization ?? null,
            status: rec.status ?? null,
            created: null,
            last_modified: null,
            source: rec.source ?? null,
            maintainers: rec.maintainers ?? null,
            abuse_contact: rec.abuseContact ?? null,
            description: rec.description ?? null,
          }
        : null,
      items,
      totalItems,
    };
  }

  async lookupPrefix(parsedPrefix: ParsedPrefix, cursorOffset: number, limit: number): Promise<PrefixLookupResult | null> {
    const isV6 = parsedPrefix.isV6;
    const allocTable = isV6 ? this.allocV6 : this.allocV4;
    let allocation: AllocationItem | null = null;
    const allocSeg = isV6
      ? findSeg(this.bytes, this.view, allocTable, (parsedPrefix.start >> 64n) >> BigInt(64 - this.allocV6B), skipAlloc)
      : findSeg4(this.bytes, this.view, allocTable, parsedPrefix.start >>> (32 - this.allocV4B), skipAlloc);

    if (allocSeg) {
      const regId = uv.read(this.bytes, allocSeg.pStart, allocSeg.pEnd);
      const ccId = regId >= 0 ? uv.read(this.bytes, uv.next, allocSeg.pEnd) : -1;
      if (ccId >= 0 && (regId !== 0 || ccId !== 0)) {
        const [startIP, endIP] = slotBoundsToStrings(isV6, allocTable.bits, allocSeg.segStart, allocSeg.nextStart, allocSeg.hasNext);
        const cc = this.countryAt(ccId);
        allocation = {
          start_ip: startIP,
          end_ip: endIP,
          version: isV6 ? 6 : 4,
          registry: this.dictAt(this.registries, regId),
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

    const scan = isV6
      ? await scanRoutesV6(this, parsedPrefix, cursorOffset, limit)
      : await scanRoutesV4(this, parsedPrefix, cursorOffset, limit);
    if (scan.tooBroad) return { tooBroad: true };
    if (!allocation && scan.routes.length === 0) return null;
    return { allocation, routes: scan.routes, nextCursor: scan.nextCursor };
  }
}
