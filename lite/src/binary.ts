import { formatIPv4, formatIPv6 } from "./ip.ts";
import type { ChunkBuffer, ChunkedSection, LazyDict, OffsetDict, SegTable } from "./types.ts";

export class UvCursor {
  next = 0;

  read(bytes: Uint8Array, pos: number, end: number): number {
    let x = 0;
    let mul = 1;
    let p = pos;
    for (let i = 0; p < end && i < 10; i++) {
      const b = bytes[p++] ?? 0;
      if (b < 0x80) {
        this.next = p;
        return x + b * mul;
      }
      x += (b & 0x7f) * mul;
      mul *= 128;
    }
    return -1;
  }
}

export function readUvarintBig(bytes: Uint8Array, pos: number, end: number): [bigint, number] | null {
  let x = 0n;
  let s = 0n;
  let p = pos;
  for (let i = 0; p < end && i < 10; i++) {
    const b = bytes[p++] ?? 0;
    if (b < 0x80) return [x + (BigInt(b) << s), p];
    x += BigInt(b & 0x7f) << s;
    s += 7n;
  }
  return null;
}

export function readOffsetDict(view: DataView, pos: number): [OffsetDict, number] {
  const count = view.getUint32(pos, true);
  const offsetsOff = pos + 4;
  const dataOff = offsetsOff + (count + 1) * 4;
  const totalBytes = view.getUint32(offsetsOff + count * 4, true);
  return [{ isOffset: true, count, offsetsOff, dataOff }, dataOff + totalBytes];
}

export function readLazyDict(bytes: Uint8Array, pos: number): [LazyDict, number] {
  const uv = new UvCursor();
  const count = uv.read(bytes, pos, bytes.length);
  if (count < 0) throw new Error("bad dict count");
  let p = uv.next;
  const offs = new Uint32Array(count);
  const lens = new Uint32Array(count);
  for (let i = 0; i < count; i++) {
    const len = uv.read(bytes, p, bytes.length);
    if (len < 0) throw new Error("bad string len");
    p = uv.next;
    offs[i] = p;
    lens[i] = len;
    p += len;
  }
  return [{ isOffset: false, offs, lens }, p];
}

export function getKeyNum(bytes: Uint8Array, off: number, kb: number): number {
  let v = 0;
  for (let i = kb - 1; i >= 0; i--) {
    v = v * 256 + (bytes[off + i] ?? 0);
  }
  return v;
}

export function getKeyBig(bytes: Uint8Array, off: number, kb: number): bigint {
  let v = 0n;
  for (let i = kb - 1; i >= 0; i--) {
    v = (v << 8n) | BigInt(bytes[off + i] ?? 0);
  }
  return v;
}

export function openSegTable(
  view: DataView,
  pos: number,
  bits: number,
  blockSize: number,
  isV6: boolean,
  hasShifted: boolean
): [SegTable, number] {
  const entries = view.getUint32(pos, true);
  const dataLen = view.getUint32(pos + 4, true);
  const start = pos + 8;
  const kb = (bits + 7) >> 3;
  const blocks = Math.ceil(entries / blockSize);
  const indexLen = blocks * (kb + 4);
  const shift = hasShifted ? (isV6 && bits === 64 ? 16 : !isV6 && bits === 32 ? 8 : 0) : 0;
  return [
    { bits, kb, stride: kb + 4, shift, entries, blocks, indexOff: start, dataOff: start + indexLen, dataLen },
    start + indexLen + dataLen,
  ];
}

export function decodeDelta(table: SegTable, encoded: bigint): bigint {
  if (table.shift === 0) return encoded;
  return (encoded & 1n) === 0n ? (encoded >> 1n) << BigInt(table.shift) : encoded >> 1n;
}

export function openChunkedSection(view: DataView, pos: number, itemCount: number): [ChunkedSection, number] {
  const chunkSize = view.getUint16(pos, true);
  let p = pos + 2;
  const numChunks = Math.ceil(itemCount / chunkSize);
  const offsetsOff = p;
  p += (numChunks + 1) * 4;
  const dataOff = p;
  const totalBytes = view.getUint32(offsetsOff + numChunks * 4, true);
  return [{ chunkSize, numChunks, offsetsOff, dataOff, cache: new Map() }, p + totalBytes];
}

export async function inflateRaw(compressed: Uint8Array): Promise<ChunkBuffer> {
  const copy = new Uint8Array(compressed);
  const stream = new Response(copy).body?.pipeThrough(new DecompressionStream("deflate-raw"));
  if (!stream) throw new Error("Missing decompression stream");
  const buf = await new Response(stream).arrayBuffer();
  const bytes = new Uint8Array(buf);
  return { bytes, view: new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength) };
}

export function skipVarint(bytes: Uint8Array, pos: number, end: number): number {
  let p = pos;
  for (let i = 0; p < end && i < 10; i++) {
    if ((bytes[p++] ?? 0) < 0x80) return p;
  }
  return -1;
}

export function skipAlloc(bytes: Uint8Array, pos: number, end: number): number {
  const p = skipVarint(bytes, pos, end);
  return p < 0 ? -1 : skipVarint(bytes, p, end);
}

export function skipGeo(bytes: Uint8Array, pos: number, end: number): number {
  return skipVarint(bytes, pos, end);
}

export function skipRoute(bytes: Uint8Array, pos: number, end: number): number {
  if (pos >= end) return -1;
  return (bytes[pos] ?? 0) === 0 ? pos + 1 : skipVarint(bytes, pos + 1, end);
}

export function slotBoundsToStrings(
  isV6: boolean,
  bits: number,
  startSlot: number | bigint,
  nextSlot: number | bigint,
  hasNext: boolean
): [string, string] {
  if (!isV6) {
    const shift = 32 - bits;
    const sNum = Number(startSlot);
    const nNum = Number(nextSlot);
    const s = shift >= 32 ? 0 : (sNum << shift) >>> 0;
    const e = hasNext ? (((shift >= 32 ? 0 : nNum << shift) - 1) >>> 0) : 0xffffffff;
    return [formatIPv4(s), formatIPv4(e)];
  }
  const shift = BigInt(64 - bits);
  const s = (BigInt(startSlot) << shift) << 64n;
  const eHi = hasNext ? (BigInt(nextSlot) << shift) - 1n : 0xffffffffffffffffn;
  const e = (eHi << 64n) | 0xffffffffffffffffn;
  return [formatIPv6(s), formatIPv6(e)];
}
