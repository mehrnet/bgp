import { UvCursor, decodeDelta, getKeyBig, getKeyNum, readUvarintBig, skipRoute } from "./binary.ts";
import { maskIPv4Range, maskIPv6Range } from "./ip.ts";
import type { LiteDB } from "./mnl1.ts";
import { findBlock, findBlock4 } from "./seg.ts";
import type { AutnumRecord, ParsedPrefixV4, ParsedPrefixV6, RouteItem } from "./types.ts";

export interface PrefixScanOutput {
  tooBroad?: boolean;
  routes: RouteItem[];
  nextCursor: string | null;
}

const MAX_SCAN = 20000;

async function emitRoute(
  db: LiteDB,
  version: 4 | 6,
  cidr: string,
  bits: number,
  targetBits: number,
  rawRouteVal: number,
  cursorOffset: number,
  limit: number,
  seen: Set<string>,
  autnumCache: Map<number, AutnumRecord>,
  routes: RouteItem[],
  state: { matched: number; nextCursor: string | null }
): Promise<boolean> {
  const indices = db.resolveASNIndices(rawRouteVal);
  for (const idx of indices) {
    const asNumber = db.view.getUint32(db.asnNumbersOff + (idx - 1) * 4, true);
    const key = `${cidr}|${asNumber}`;
    if (seen.has(key)) continue;
    seen.add(key);
    if (state.matched >= cursorOffset) {
      if (routes.length >= limit) {
        state.nextCursor = String(state.matched);
        return true;
      }
      let a = autnumCache.get(idx);
      if (!a) {
        a = db.autnumChunks ? await db.autnumByIdx(idx, false) : db.autnumByIdxSync(idx, false);
        autnumCache.set(idx, a);
      }
      const relation = bits === targetBits ? "exact" : bits < targetBits ? "covering" : "more_specific";
      routes.push({
        prefix: cidr,
        version,
        origin_asn: `AS${asNumber}`,
        as_number: asNumber,
        relation,
        registry: a.hasAutnum ? (a.registry ?? null) : null,
        source: a.hasAutnum ? (a.source ?? null) : null,
        maintainers: null,
        organization: a.hasAutnum ? (a.organization ?? null) : null,
        abuse_contact: null,
        description: null,
      });
    }
    state.matched++;
  }
  return false;
}

export async function scanRoutesV4(
  db: LiteDB,
  parsedPrefix: ParsedPrefixV4,
  cursorOffset: number,
  limit: number
): Promise<PrefixScanOutput> {
  const table = db.routeV4;
  const routes: RouteItem[] = [];
  const seen = new Set<string>();
  const autnumCache = new Map<number, AutnumRecord>();
  const state = { matched: 0, nextCursor: null as string | null };
  const uv = new UvCursor();
  const startNum = parsedPrefix.start >>> 0;
  const endNum = parsedPrefix.end >>> 0;
  let scanned = 0;
  let b = findBlock4(db.bytes, table, startNum);
  if (b < 0) b = 0;
  const shift = table.shift;
  const shiftMul = shift === 0 ? 1 : 1 << shift;

  outer4: for (; b < table.blocks; b++) {
    let curKey = getKeyNum(db.bytes, table.indexOff + b * table.stride, table.kb);
    let off = table.dataOff + db.view.getUint32(table.indexOff + b * table.stride + table.kb, true);
    const end = b + 1 < table.blocks
      ? table.dataOff + db.view.getUint32(table.indexOff + (b + 1) * table.stride + table.kb, true)
      : table.dataOff + table.dataLen;
    while (off < end) {
      if (++scanned > MAX_SCAN) return { tooBroad: true, routes: [], nextCursor: null };
      const b0 = db.bytes[off] ?? 0;
      let enc: number;
      let pStart: number;
      if (b0 < 0x80) {
        enc = b0;
        pStart = off + 1;
      } else {
        enc = uv.read(db.bytes, off, end);
        if (enc < 0) break outer4;
        pStart = uv.next;
      }
      const delta = shift === 0 ? enc : enc % 2 === 0 ? Math.floor(enc / 2) * shiftMul : Math.floor(enc / 2);
      curKey += delta;
      const nextOff = skipRoute(db.bytes, pStart, end);
      if (nextOff < 0 || curKey > endNum) break outer4;

      const bits = db.bytes[pStart] ?? 0;
      if (bits > 0) {
        const rVal = uv.read(db.bytes, pStart + 1, nextOff);
        const rawRouteVal = rVal >= 0 ? db.resolveRouteVal(rVal) : 0;
        if (rawRouteVal !== 0) {
          const masked = maskIPv4Range(curKey >>> 0, bits);
          if (masked.end >= startNum && masked.start <= endNum) {
            const stop = await emitRoute(db, 4, masked.cidr, bits, parsedPrefix.bits, rawRouteVal, cursorOffset, limit, seen, autnumCache, routes, state);
            if (stop) break outer4;
          }
        }
      }
      off = nextOff;
    }
  }
  return { routes, nextCursor: state.nextCursor };
}

export async function scanRoutesV6(
  db: LiteDB,
  parsedPrefix: ParsedPrefixV6,
  cursorOffset: number,
  limit: number
): Promise<PrefixScanOutput> {
  const table = db.routeV6;
  const routes: RouteItem[] = [];
  const seen = new Set<string>();
  const autnumCache = new Map<number, AutnumRecord>();
  const state = { matched: 0, nextCursor: null as string | null };
  const uv = new UvCursor();
  const startKey = parsedPrefix.start >> 64n;
  const endKey = parsedPrefix.end >> 64n;
  let scanned = 0;
  let b = findBlock(db.bytes, table, startKey);
  if (b < 0) b = 0;

  outer6: for (; b < table.blocks; b++) {
    let curKey = getKeyBig(db.bytes, table.indexOff + b * table.stride, table.kb);
    let off = table.dataOff + db.view.getUint32(table.indexOff + b * table.stride + table.kb, true);
    const end = b + 1 < table.blocks
      ? table.dataOff + db.view.getUint32(table.indexOff + (b + 1) * table.stride + table.kb, true)
      : table.dataOff + table.dataLen;
    while (off < end) {
      if (++scanned > MAX_SCAN) return { tooBroad: true, routes: [], nextCursor: null };
      const d = readUvarintBig(db.bytes, off, end);
      if (!d) break outer6;
      curKey += decodeDelta(table, d[0]);
      const pStart = d[1];
      const nextOff = skipRoute(db.bytes, pStart, end);
      if (nextOff < 0 || curKey > endKey) break outer6;

      const bits = db.bytes[pStart] ?? 0;
      if (bits > 0) {
        const rVal = uv.read(db.bytes, pStart + 1, nextOff);
        const rawRouteVal = rVal >= 0 ? db.resolveRouteVal(rVal) : 0;
        if (rawRouteVal !== 0) {
          const masked = maskIPv6Range(curKey << 64n, bits);
          if (masked.end >= parsedPrefix.start && masked.start <= parsedPrefix.end) {
            const stop = await emitRoute(db, 6, masked.cidr, bits, parsedPrefix.bits, rawRouteVal, cursorOffset, limit, seen, autnumCache, routes, state);
            if (stop) break outer6;
          }
        }
      }
      off = nextOff;
    }
  }
  return { routes, nextCursor: state.nextCursor };
}
