import { decodeDelta, getKeyBig, getKeyNum, readUvarintBig } from "./binary.ts";
import type { SegTable } from "./types.ts";

export interface SegMatch<K> {
  segStart: K;
  nextStart: K;
  hasNext: boolean;
  pStart: number;
  pEnd: number;
}

export type PayloadSkipper = (bytes: Uint8Array, pos: number, end: number) => number;

export function findBlock4(bytes: Uint8Array, table: SegTable, keyNum: number): number {
  let lo = 0;
  let hi = table.blocks;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    const k = getKeyNum(bytes, table.indexOff + mid * table.stride, table.kb);
    if (k > keyNum) hi = mid;
    else lo = mid + 1;
  }
  return lo - 1;
}

export function findSeg4(
  bytes: Uint8Array,
  view: DataView,
  table: SegTable,
  keyNum: number,
  skipPayload: PayloadSkipper
): SegMatch<number> | null {
  const b = findBlock4(bytes, table, keyNum);
  if (b < 0) return null;
  let curKey = getKeyNum(bytes, table.indexOff + b * table.stride, table.kb);
  let off = table.dataOff + view.getUint32(table.indexOff + b * table.stride + table.kb, true);
  const end =
    b + 1 < table.blocks
      ? table.dataOff + view.getUint32(table.indexOff + (b + 1) * table.stride + table.kb, true)
      : table.dataOff + table.dataLen;

  const shift = table.shift;
  const shiftMul = shift === 0 ? 1 : 1 << shift;
  let prevStart = 0;
  let prevPStart = 0;
  let prevPEnd = 0;
  let found = false;

  while (off < end) {
    const b0 = bytes[off] ?? 0;
    let enc = 0;
    let pStart = off + 1;
    if (b0 < 0x80) {
      enc = b0;
    } else {
      let mul = 1;
      pStart = off;
      let ok = false;
      for (let i = 0; pStart < end && i < 10; i++) {
        const byte = bytes[pStart++] ?? 0;
        if (byte < 0x80) { enc += byte * mul; ok = true; break; }
        enc += (byte & 0x7f) * mul;
        mul *= 128;
      }
      if (!ok) return null;
    }
    const delta = shift === 0 ? enc : enc % 2 === 0 ? Math.floor(enc / 2) * shiftMul : Math.floor(enc / 2);
    curKey += delta;
    const nextOff = skipPayload(bytes, pStart, end);
    if (nextOff < 0) return null;
    if (curKey > keyNum) {
      return found ? { segStart: prevStart, nextStart: curKey, hasNext: true, pStart: prevPStart, pEnd: prevPEnd } : null;
    }
    prevStart = curKey;
    prevPStart = pStart;
    prevPEnd = nextOff;
    found = true;
    off = nextOff;
  }
  if (!found) return null;
  if (b + 1 < table.blocks) {
    const nextBlockKey = getKeyNum(bytes, table.indexOff + (b + 1) * table.stride, table.kb);
    return { segStart: prevStart, nextStart: nextBlockKey, hasNext: true, pStart: prevPStart, pEnd: prevPEnd };
  }
  return { segStart: prevStart, nextStart: 0, hasNext: false, pStart: prevPStart, pEnd: prevPEnd };
}

export function findBlock(bytes: Uint8Array, table: SegTable, keyBig: bigint): number {
  let lo = 0;
  let hi = table.blocks;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    const k = getKeyBig(bytes, table.indexOff + mid * table.stride, table.kb);
    if (k > keyBig) hi = mid;
    else lo = mid + 1;
  }
  return lo - 1;
}

export function findSeg(
  bytes: Uint8Array,
  view: DataView,
  table: SegTable,
  keyBig: bigint,
  skipPayload: PayloadSkipper
): SegMatch<bigint> | null {
  const b = findBlock(bytes, table, keyBig);
  if (b < 0) return null;
  let curKey = getKeyBig(bytes, table.indexOff + b * table.stride, table.kb);
  let off = table.dataOff + view.getUint32(table.indexOff + b * table.stride + table.kb, true);
  const end =
    b + 1 < table.blocks
      ? table.dataOff + view.getUint32(table.indexOff + (b + 1) * table.stride + table.kb, true)
      : table.dataOff + table.dataLen;

  let prevStart = 0n;
  let prevPStart = 0;
  let prevPEnd = 0;
  let found = false;

  while (off < end) {
    const d = readUvarintBig(bytes, off, end);
    if (!d) return null;
    curKey += decodeDelta(table, d[0]);
    const pStart = d[1];
    const nextOff = skipPayload(bytes, pStart, end);
    if (nextOff < 0) return null;
    if (curKey > keyBig) {
      return found ? { segStart: prevStart, nextStart: curKey, hasNext: true, pStart: prevPStart, pEnd: prevPEnd } : null;
    }
    prevStart = curKey;
    prevPStart = pStart;
    prevPEnd = nextOff;
    found = true;
    off = nextOff;
  }
  if (!found) return null;
  if (b + 1 < table.blocks) {
    const nextBlockKey = getKeyBig(bytes, table.indexOff + (b + 1) * table.stride, table.kb);
    return { segStart: prevStart, nextStart: nextBlockKey, hasNext: true, pStart: prevPStart, pEnd: prevPEnd };
  }
  return { segStart: prevStart, nextStart: 0n, hasNext: false, pStart: prevPStart, pEnd: prevPEnd };
}
