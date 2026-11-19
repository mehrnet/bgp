import type { ParsedIP, ParsedPrefix } from "./types.ts";

const V4_MAPPED_PREFIX = 0xffff00000000n;
const V4_MAPPED_MASK = ~0xffffffffn;

export function parseIPv4(input: string): number | null {
  const len = input.length;
  if (len < 7 || len > 15) return null;
  let val = 0;
  let dots = 0;
  let part = 0;
  let partLen = 0;
  let leadingZero = false;
  for (let i = 0; i < len; i++) {
    const ch = input.charCodeAt(i);
    if (ch === 46) {
      if (partLen === 0 || dots === 3) return null;
      val = (val << 8) | part;
      dots++;
      part = 0;
      partLen = 0;
      leadingZero = false;
    } else if (ch >= 48 && ch <= 57) {
      if (partLen === 0) leadingZero = ch === 48;
      else if (leadingZero || partLen === 3) return null;
      part = part * 10 + (ch - 48);
      if (part > 255) return null;
      partLen++;
    } else {
      return null;
    }
  }
  if (dots !== 3 || partLen === 0) return null;
  return ((val << 8) | part) >>> 0;
}

export function formatIPv4(val: number): string {
  const u = val >>> 0;
  return `${(u >>> 24) & 255}.${(u >>> 16) & 255}.${(u >>> 8) & 255}.${u & 255}`;
}

export function parseIPv6(input: string): bigint | null {
  if (!input.includes(":") || input.includes("%")) return null;
  let norm = input;
  let v4Suffix: [number, number] | null = null;
  const lastColon = norm.lastIndexOf(":");
  if (lastColon !== -1 && norm.slice(lastColon + 1).includes(".")) {
    const v4 = parseIPv4(norm.slice(lastColon + 1));
    if (v4 === null) return null;
    v4Suffix = [(v4 >>> 16) & 0xffff, v4 & 0xffff];
    norm = `${norm.slice(0, lastColon + 1)}0:0`;
  }

  const dbl = norm.indexOf("::");
  let head: string[] = [];
  let tail: string[] = [];
  if (dbl !== -1) {
    if (norm.indexOf("::", dbl + 2) !== -1) return null;
    const left = norm.slice(0, dbl);
    const right = norm.slice(dbl + 2);
    head = left === "" ? [] : left.split(":");
    tail = right === "" ? [] : right.split(":");
    if (head.length + tail.length > 7) return null;
  } else {
    head = norm.split(":");
    if (head.length !== 8) return null;
  }

  const groups = new Uint16Array(8);
  const parseGroup = (s: string): number => (/^[0-9a-fA-F]{1,4}$/.test(s) ? parseInt(s, 16) : -1);
  for (let i = 0; i < head.length; i++) {
    const g = parseGroup(head[i] ?? "");
    if (g < 0) return null;
    groups[i] = g;
  }
  const tailStart = 8 - tail.length;
  for (let i = 0; i < tail.length; i++) {
    const g = parseGroup(tail[i] ?? "");
    if (g < 0) return null;
    groups[tailStart + i] = g;
  }
  if (v4Suffix !== null) {
    groups[6] = v4Suffix[0];
    groups[7] = v4Suffix[1];
  }

  const hi = (BigInt(groups[0] ?? 0) << 48n) | (BigInt(groups[1] ?? 0) << 32n) | (BigInt(groups[2] ?? 0) << 16n) | BigInt(groups[3] ?? 0);
  const lo = (BigInt(groups[4] ?? 0) << 48n) | (BigInt(groups[5] ?? 0) << 32n) | (BigInt(groups[6] ?? 0) << 16n) | BigInt(groups[7] ?? 0);
  return (hi << 64n) | lo;
}

export function formatIPv6(val: bigint): string {
  const groups = new Array<number>(8);
  let cur = val;
  for (let i = 7; i >= 0; i--) {
    groups[i] = Number(cur & 0xffffn);
    cur >>= 16n;
  }
  let bestStart = -1;
  let bestLen = 0;
  let curStart = -1;
  let curLen = 0;
  for (let i = 0; i < 8; i++) {
    if (groups[i] === 0) {
      if (curStart === -1) { curStart = i; curLen = 1; }
      else curLen++;
      if (curLen > bestLen) { bestStart = curStart; bestLen = curLen; }
    } else {
      curStart = -1;
      curLen = 0;
    }
  }
  if (bestLen < 2) bestStart = -1;

  const parts: string[] = [];
  for (let i = 0; i < 8; i++) {
    if (i === bestStart) {
      parts.push("");
      if (i === 0) parts.push("");
      i += bestLen - 1;
      if (i === 7) parts.push("");
    } else {
      parts.push((groups[i] ?? 0).toString(16));
    }
  }
  return parts.join(":");
}

export function parseIP(raw: string): ParsedIP | null {
  const s = raw.trim();
  if (!s) return null;
  const v4 = parseIPv4(s);
  if (v4 !== null) return { isV6: false, value: v4, canonical: formatIPv4(v4) };
  const v6 = parseIPv6(s);
  if (v6 === null) return null;
  if ((v6 & V4_MAPPED_MASK) === V4_MAPPED_PREFIX) {
    const unmapped = Number(v6 & 0xffffffffn) >>> 0;
    return { isV6: false, value: unmapped, canonical: formatIPv4(unmapped) };
  }
  return { isV6: true, value: v6, canonical: formatIPv6(v6) };
}

export function maskIPv4Range(addr: number, bits: number): { start: number; end: number; cidr: string } {
  if (bits === 0) return { start: 0, end: 0xffffffff >>> 0, cidr: "0.0.0.0/0" };
  const mask = (~0 << (32 - bits)) >>> 0;
  const start = (addr & mask) >>> 0;
  const end = (start | (~mask >>> 0)) >>> 0;
  return { start, end, cidr: `${formatIPv4(start)}/${bits}` };
}

export function maskIPv6Range(addr: bigint, bits: number): { start: bigint; end: bigint; cidr: string } {
  if (bits === 0) return { start: 0n, end: (1n << 128n) - 1n, cidr: "::/0" };
  const shift = BigInt(128 - bits);
  const start = (addr >> shift) << shift;
  const end = start | ((1n << shift) - 1n);
  return { start, end, cidr: `${formatIPv6(start)}/${bits}` };
}

export function parsePrefix(raw: string): ParsedPrefix | null {
  const s = raw.trim();
  const slash = s.indexOf("/");
  if (slash <= 0) return null;
  const ipStr = s.slice(0, slash);
  const lenStr = s.slice(slash + 1);
  if (!/^(0|[1-9]\d{0,2})$/.test(lenStr)) return null;
  const bits = Number(lenStr);

  const v4 = parseIPv4(ipStr);
  if (v4 !== null) {
    if (bits < 0 || bits > 32) return null;
    const { start, end, cidr } = maskIPv4Range(v4, bits);
    return { isV6: false, version: 4, bits, cidr, start, end, startIP: formatIPv4(start), endIP: formatIPv4(end), addressCount: (1n << BigInt(32 - bits)).toString() };
  }
  const v6 = parseIPv6(ipStr);
  if (v6 === null || bits < 0 || bits > 128) return null;
  if ((v6 & V4_MAPPED_MASK) === V4_MAPPED_PREFIX && bits >= 96) {
    const mappedBits = bits - 96;
    const mappedV4 = Number(v6 & 0xffffffffn) >>> 0;
    const { start, end, cidr } = maskIPv4Range(mappedV4, mappedBits);
    return { isV6: false, version: 4, bits: mappedBits, cidr, start, end, startIP: formatIPv4(start), endIP: formatIPv4(end), addressCount: (1n << BigInt(32 - mappedBits)).toString() };
  }
  const { start, end, cidr } = maskIPv6Range(v6, bits);
  return { isV6: true, version: 6, bits, cidr, start, end, startIP: formatIPv6(start), endIP: formatIPv6(end), addressCount: (1n << BigInt(128 - bits)).toString() };
}
