import { openSegTable, skipRoute, UvCursor } from "../../../lite/src/binary.ts";
import { formatIPv4, formatIPv6, maskIPv4Range, maskIPv6Range, parseIP } from "../../../lite/src/ip.ts";
import { findSeg, findSeg4 } from "../../../lite/src/seg.ts";
import type { SegTable } from "../../../lite/src/types.ts";
import { createLazyDb, readMetaJson } from "./decode.ts";

export interface AsnLookup {
  cidr: string;
  asn: string | null;
  as_number: number | null;
  asns: string[];
  as_numbers: number[];
}

interface AsnRouteDb {
  bytes: Uint8Array;
  view: DataView;
  meta: Record<string, string | null>;
  asnCount: number;
  asnNumbersOff: number;
  multiCount: number;
  multiOffsetsOff: number;
  multiDataOff: number;
  routeValCount: number;
  routeValOff: number;
  routeV4: SegTable;
  routeV6: SegTable;
}

const uv = new UvCursor();

function parseAsnRouteDb(bytes: Uint8Array, view: DataView): AsnRouteDb {
  if (bytes.length < 11 || bytes[0] !== 0x4d || bytes[1] !== 0x4e || bytes[2] !== 0x41 || bytes[3] !== 0x31 || bytes[4] !== 1) {
    throw new Error("invalid MNA1 database");
  }
  const flags = bytes[5] ?? 0;
  const blockSize = bytes[6] ?? 64;
  const v4Bits = bytes[7] ?? 32;
  const v6Bits = bytes[8] ?? 64;
  let pos: number;
  let meta: Record<string, string | null>;
  [meta, pos] = readMetaJson(bytes, view, 9);

  const asnCount = view.getUint32(pos, true);
  pos += 4;
  const asnNumbersOff = pos;
  pos += asnCount * 4;

  const multiCount = view.getUint32(pos, true);
  pos += 4;
  const multiOffsetsOff = pos;
  pos += (multiCount + 1) * 4;
  const multiDataOff = pos;
  pos += view.getUint32(multiOffsetsOff + multiCount * 4, true);

  let routeValCount = 0;
  let routeValOff = -1;
  if ((flags & 8) !== 0) {
    routeValCount = view.getUint32(pos, true);
    pos += 4;
    routeValOff = pos;
    pos += routeValCount * 4;
  }

  const hasShifted = (flags & 2) !== 0;
  let routeV4: SegTable;
  let routeV6: SegTable;
  [routeV4, pos] = openSegTable(view, pos, v4Bits, blockSize, false, hasShifted);
  [routeV6, pos] = openSegTable(view, pos, v6Bits, blockSize, true, hasShifted);
  return { bytes, view, meta, asnCount, asnNumbersOff, multiCount, multiOffsetsOff, multiDataOff, routeValCount, routeValOff, routeV4, routeV6 };
}

function resolveAsNumbers(db: AsnRouteDb, rVal: number): number[] {
  const val = rVal === 0 || db.routeValCount === 0 ? rVal : rVal > db.routeValCount ? 0 : db.view.getUint32(db.routeValOff + (rVal - 1) * 4, true);
  if (val === 0) return [];
  if (val <= db.asnCount) return [db.view.getUint32(db.asnNumbersOff + (val - 1) * 4, true)];
  const mIdx = val - db.asnCount - 1;
  if (mIdx < 0 || mIdx >= db.multiCount) return [];
  const off = db.multiDataOff + db.view.getUint32(db.multiOffsetsOff + mIdx * 4, true);
  const end = db.multiDataOff + db.view.getUint32(db.multiOffsetsOff + (mIdx + 1) * 4, true);
  const count = uv.read(db.bytes, off, end);
  if (count <= 0) return [];
  let p = uv.next;
  const out: number[] = [];
  for (let i = 0; i < count; i++) {
    const idx = uv.read(db.bytes, p, end);
    if (idx <= 0 || idx > db.asnCount) break;
    out.push(db.view.getUint32(db.asnNumbersOff + (idx - 1) * 4, true));
    p = uv.next;
  }
  return out;
}

const getDb = createLazyDb(parseAsnRouteDb);

export function init(): Promise<void> {
  return getDb().then(() => undefined);
}

export async function getMeta(): Promise<Record<string, string | null>> {
  return (await getDb()).meta;
}

export async function lookupAsn(ip: string): Promise<AsnLookup | null> {
  const parsed = parseIP(ip);
  if (!parsed) return null;
  const db = await getDb();
  const seg = parsed.isV6
    ? findSeg(db.bytes, db.view, db.routeV6, (parsed.value >> 64n) >> BigInt(64 - db.routeV6.bits), skipRoute)
    : findSeg4(db.bytes, db.view, db.routeV4, parsed.value >>> 0, skipRoute);
  if (!seg || seg.pStart >= seg.pEnd) return null;
  const bits = db.bytes[seg.pStart] ?? 0;
  if (bits === 0) return null;
  const rVal = uv.read(db.bytes, seg.pStart + 1, seg.pEnd);
  if (rVal <= 0) return null;
  const asNumbers = resolveAsNumbers(db, rVal);
  if (asNumbers.length === 0) return null;
  const cidr = parsed.isV6
    ? `${formatIPv6(maskIPv6Range(parsed.value, bits).start)}/${bits}`
    : `${formatIPv4(maskIPv4Range(parsed.value, bits).start)}/${bits}`;
  const asns = asNumbers.map((n) => `AS${n}`);
  const firstNum = asNumbers.length === 1 ? (asNumbers[0] ?? null) : null;
  const firstAsn = asns.length === 1 ? (asns[0] ?? null) : null;
  return { cidr, asn: firstAsn, as_number: firstNum, asns, as_numbers: asNumbers };
}
