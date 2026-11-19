import { UvCursor, skipVarint } from "../../lite/src/binary.ts";
import { LiteDB } from "../../lite/src/mnl1.ts";

const encoder = new TextEncoder();

function writeU32(out: number[], val: number): void {
  out.push(val & 0xff, (val >>> 8) & 0xff, (val >>> 16) & 0xff, (val >>> 24) & 0xff);
}

function writeU16(out: number[], val: number): void {
  out.push(val & 0xff, (val >>> 8) & 0xff);
}

function writeUvarint(out: number[], val: number): void {
  let v = val >>> 0;
  while (v >= 0x80) {
    out.push((v & 0x7f) | 0x80);
    v >>>= 7;
  }
  out.push(v);
}

function concatChunks(chunks: Uint8Array[]): Uint8Array {
  let total = 0;
  for (const c of chunks) total += c.length;
  const out = new Uint8Array(total);
  let pos = 0;
  for (const c of chunks) {
    out.set(c, pos);
    pos += c.length;
  }
  return out;
}

export function buildMna1(liteBytes: Uint8Array, db: LiteDB): Uint8Array {
  const metaBytes = encoder.encode(JSON.stringify(db.meta));
  const flags = db.flags & (2 | 8);
  const head: number[] = [
    0x4d, 0x4e, 0x41, 0x31, // MNA1
    1,
    flags,
    db.blockSize,
    db.routeV4B,
    db.routeV6B,
  ];
  writeU16(head, metaBytes.length);

  const asnHeader: number[] = [];
  writeU32(asnHeader, db.asnCount);
  const asnNumbers = liteBytes.subarray(db.asnNumbersOff, db.asnNumbersOff + db.asnCount * 4);

  const multiTotal = db.view.getUint32(db.multiOffsetsOff + db.multiCount * 4, true);
  const multiSlice = liteBytes.subarray(db.multiOffsetsOff - 4, db.multiDataOff + multiTotal);

  const routeFreqSlice =
    (flags & 8) !== 0
      ? liteBytes.subarray(db.routeValOff - 4, db.routeValOff + db.routeValCount * 4)
      : new Uint8Array(0);

  const routeV4Slice = liteBytes.subarray(db.routeV4.indexOff - 8, db.routeV4.dataOff + db.routeV4.dataLen);
  const routeV6Slice = liteBytes.subarray(db.routeV6.indexOff - 8, db.routeV6.dataOff + db.routeV6.dataLen);

  return concatChunks([
    new Uint8Array(head),
    metaBytes,
    new Uint8Array(asnHeader),
    asnNumbers,
    multiSlice,
    routeFreqSlice,
    routeV4Slice,
    routeV6Slice,
  ]);
}

function packOffsetDict(db: LiteDB): Uint8Array {
  const dict = db.registries;
  if (dict.isOffset) {
    const total = db.view.getUint32(dict.offsetsOff + dict.count * 4, true);
    return db.bytes.subarray(dict.offsetsOff - 4, dict.dataOff + total);
  }
  const count = dict.lens.length;
  const header: number[] = [];
  writeU32(header, count);
  let running = 0;
  for (let i = 0; i < count; i++) {
    writeU32(header, running);
    running += dict.lens[i] ?? 0;
  }
  writeU32(header, running);
  const chunks: Uint8Array[] = [new Uint8Array(header)];
  for (let i = 0; i < count; i++) {
    const off = dict.offs[i] ?? 0;
    const len = dict.lens[i] ?? 0;
    chunks.push(db.bytes.subarray(off, off + len));
  }
  return concatChunks(chunks);
}

export async function buildMnm1(liteBytes: Uint8Array, db: LiteDB): Promise<Uint8Array> {
  const metaBytes = encoder.encode(JSON.stringify(db.meta));
  const head: number[] = [0x4d, 0x4e, 0x4d, 0x31, 1]; // MNM1 v1
  writeU16(head, metaBytes.length);

  const countriesSlice = liteBytes.subarray(db.countriesOff - 2, db.countriesOff + db.ccCount * 2);
  const registriesSlice = packOffsetDict(db);

  const asnHeader: number[] = [];
  writeU32(asnHeader, db.asnCount);
  const asnNumbers = liteBytes.subarray(db.asnNumbersOff, db.asnNumbersOff + db.asnCount * 4);

  const uv = new UvCursor();
  const offsets: number[] = [];
  const data: number[] = [];

  for (let i = 0; i < db.asnCount; i++) {
    writeU32(offsets, data.length);
    const entry = await db.readChunkEntry(db.autnumChunks, db.asnOffsetsOff, db.asnDataOff, i);
    const { bytes, off, end } = entry;
    if (off >= end || (bytes[off] ?? 0) === 0) {
      data.push(0);
      continue;
    }
    const r1 = uv.read(bytes, off + 1, end);
    const r2 = uv.read(bytes, uv.next, end);
    let p = skipVarint(bytes, uv.next, end); // skip status
    if (p >= 0) p = skipVarint(bytes, p, end); // skip source
    if (r1 < 0 || r2 < 0 || p < 0) {
      data.push(0);
      continue;
    }
    const nameLen = uv.read(bytes, p, end);
    if (nameLen < 0) {
      data.push(0);
      continue;
    }
    const nameStart = uv.next;
    const orgLen = uv.read(bytes, nameStart + nameLen, end);
    if (orgLen < 0) {
      data.push(0);
      continue;
    }
    const orgStart = uv.next;

    data.push(1);
    writeUvarint(data, r1);
    writeUvarint(data, r2);
    writeUvarint(data, nameLen);
    for (let j = 0; j < nameLen; j++) data.push(bytes[nameStart + j] ?? 0);
    writeUvarint(data, orgLen);
    for (let j = 0; j < orgLen; j++) data.push(bytes[orgStart + j] ?? 0);
  }
  writeU32(offsets, data.length);

  return concatChunks([
    new Uint8Array(head),
    metaBytes,
    countriesSlice,
    registriesSlice,
    new Uint8Array(asnHeader),
    asnNumbers,
    new Uint8Array(offsets),
    new Uint8Array(data),
  ]);
}
