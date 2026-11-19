import { openSegTable, skipVarint, UvCursor } from "../../../lite/src/binary.ts";
import { parseIP } from "../../../lite/src/ip.ts";
import { findSeg, findSeg4 } from "../../../lite/src/seg.ts";
import type { SegTable } from "../../../lite/src/types.ts";
import { createLazyDb } from "./decode.ts";

interface CountryDb {
  bytes: Uint8Array;
  view: DataView;
  v4Bits: number;
  v6Bits: number;
  ccCount: number;
  countriesOff: number;
  v4Table: SegTable;
  v6Table: SegTable;
}

const uv = new UvCursor();

function parseCountryDb(bytes: Uint8Array, view: DataView): CountryDb {
  if (
    bytes.length < 10 ||
    bytes[0] !== 0x4d ||
    bytes[1] !== 0x4e ||
    bytes[2] !== 0x43 ||
    bytes[3] !== 0x43 ||
    bytes[4] !== 1
  ) {
    throw new Error("invalid MNCC database");
  }
  const v4Bits = bytes[5] ?? 24;
  const v6Bits = bytes[6] ?? 48;
  const blockSize = bytes[7] ?? 64;
  const ccCount = view.getUint16(8, true);
  const countriesOff = 10;
  let pos = countriesOff + ccCount * 2;
  let v4Table: SegTable;
  let v6Table: SegTable;
  [v4Table, pos] = openSegTable(view, pos, v4Bits, blockSize, false, false);
  [v6Table, pos] = openSegTable(view, pos, v6Bits, blockSize, true, false);
  return { bytes, view, v4Bits, v6Bits, ccCount, countriesOff, v4Table, v6Table };
}

const getDb = createLazyDb(parseCountryDb);

export function init(): Promise<void> {
  return getDb().then(() => undefined);
}

export async function lookupCountry(ip: string): Promise<string | null> {
  const parsed = parseIP(ip);
  if (!parsed) return null;
  const db = await getDb();
  const seg = parsed.isV6
    ? findSeg(db.bytes, db.view, db.v6Table, (parsed.value >> 64n) >> BigInt(64 - db.v6Bits), skipVarint)
    : findSeg4(db.bytes, db.view, db.v4Table, parsed.value >>> (32 - db.v4Bits), skipVarint);
  if (!seg) return null;
  const ccId = uv.read(db.bytes, seg.pStart, seg.pEnd);
  if (ccId <= 0 || ccId > db.ccCount) return null;
  const off = db.countriesOff + (ccId - 1) * 2;
  return String.fromCharCode(db.bytes[off] ?? 0, db.bytes[off + 1] ?? 0);
}
