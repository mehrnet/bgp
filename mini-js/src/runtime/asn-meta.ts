import { readOffsetDict, UvCursor } from "../../../lite/src/binary.ts";
import type { OffsetDict } from "../../../lite/src/types.ts";
import { createLazyDb, readMetaJson } from "./decode.ts";

export interface AsnMeta {
  asn: string;
  as_number: number;
  name: string | null;
  organization: string | null;
  registry: string | null;
  country_code: string | null;
}

interface AsnMetaDb {
  bytes: Uint8Array;
  view: DataView;
  meta: Record<string, string | null>;
  ccCount: number;
  countriesOff: number;
  registries: OffsetDict;
  asnCount: number;
  asnNumbersOff: number;
  offsetsOff: number;
  dataOff: number;
}

const decoder = new TextDecoder();
const uv = new UvCursor();

function parseAsnMetaDb(bytes: Uint8Array, view: DataView): AsnMetaDb {
  if (bytes.length < 7 || bytes[0] !== 0x4d || bytes[1] !== 0x4e || bytes[2] !== 0x4d || bytes[3] !== 0x31 || bytes[4] !== 1) {
    throw new Error("invalid MNM1 database");
  }
  let pos: number;
  let meta: Record<string, string | null>;
  [meta, pos] = readMetaJson(bytes, view, 5);
  const ccCount = view.getUint16(pos, true);
  pos += 2;
  const countriesOff = pos;
  pos += ccCount * 2;
  let registries: OffsetDict;
  [registries, pos] = readOffsetDict(view, pos);
  const asnCount = view.getUint32(pos, true);
  pos += 4;
  const asnNumbersOff = pos;
  pos += asnCount * 4;
  const offsetsOff = pos;
  const dataOff = offsetsOff + (asnCount + 1) * 4;
  return { bytes, view, meta, ccCount, countriesOff, registries, asnCount, asnNumbersOff, offsetsOff, dataOff };
}

function parseAsnInput(input: string | number): number | null {
  if (typeof input === "number") {
    return Number.isSafeInteger(input) && input >= 1 && input <= 0xffffffff ? input : null;
  }
  const s = input.trim().toUpperCase().replace(/^AS/, "");
  if (!/^[1-9]\d*$/.test(s)) return null;
  const n = Number(s);
  return Number.isSafeInteger(n) && n >= 1 && n <= 0xffffffff ? n : null;
}

function findAsnIdx(db: AsnMetaDb, asNumber: number): number {
  let lo = 0;
  let hi = db.asnCount;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    const v = db.view.getUint32(db.asnNumbersOff + mid * 4, true);
    if (v < asNumber) lo = mid + 1;
    else hi = mid;
  }
  return lo < db.asnCount && db.view.getUint32(db.asnNumbersOff + lo * 4, true) === asNumber ? lo : -1;
}

const getDb = createLazyDb(parseAsnMetaDb);

export function init(): Promise<void> {
  return getDb().then(() => undefined);
}

export async function getMeta(): Promise<Record<string, string | null>> {
  return (await getDb()).meta;
}

export async function lookupAsnMeta(asnInput: string | number): Promise<AsnMeta | null> {
  const asNumber = parseAsnInput(asnInput);
  if (asNumber === null) return null;
  const db = await getDb();
  const idx = findAsnIdx(db, asNumber);
  if (idx < 0) return null;
  const off = db.dataOff + db.view.getUint32(db.offsetsOff + idx * 4, true);
  const end = db.dataOff + db.view.getUint32(db.offsetsOff + (idx + 1) * 4, true);
  if (off >= end || (db.bytes[off] ?? 0) === 0) return null;

  const regId = uv.read(db.bytes, off + 1, end);
  const ccId = uv.read(db.bytes, uv.next, end);
  const nameLen = uv.read(db.bytes, uv.next, end);
  let p = uv.next;
  const name = nameLen > 0 ? decoder.decode(db.bytes.subarray(p, p + nameLen)) : null;
  p += nameLen;
  const orgLen = uv.read(db.bytes, p, end);
  p = uv.next;
  const organization = orgLen > 0 ? decoder.decode(db.bytes.subarray(p, p + orgLen)) : null;

  const registry = regId > 0 && regId <= db.registries.count
    ? decoder.decode(db.bytes.subarray(
        db.registries.dataOff + db.view.getUint32(db.registries.offsetsOff + (regId - 1) * 4, true),
        db.registries.dataOff + db.view.getUint32(db.registries.offsetsOff + regId * 4, true)
      ))
    : null;
  const country_code = ccId > 0 && ccId <= db.ccCount
    ? String.fromCharCode(db.bytes[db.countriesOff + (ccId - 1) * 2] ?? 0, db.bytes[db.countriesOff + (ccId - 1) * 2 + 1] ?? 0)
    : null;

  return { asn: `AS${asNumber}`, as_number: asNumber, name, organization, registry, country_code };
}
