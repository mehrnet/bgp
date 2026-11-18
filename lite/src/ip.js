// IPv4 and IPv6 parsing, canonical formatting, CIDR math, and BigInt address counts.

export function parseIPv4(input) {
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
      if (partLen === 0) {
        leadingZero = ch === 48;
      } else if (leadingZero || partLen === 3) {
        return null;
      }
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

export function formatIPv4(val) {
  val = val >>> 0;
  return `${(val >>> 24) & 255}.${(val >>> 16) & 255}.${(val >>> 8) & 255}.${val & 255}`;
}

export function parseIPv6(input) {
  if (!input.includes(":")) return null;
  let zoneIdx = input.indexOf("%");
  if (zoneIdx !== -1) return null;

  // Handle IPv4-embedded suffix (e.g. ::ffff:1.2.3.4)
  let v4Suffix = null;
  const lastColon = input.lastIndexOf(":");
  if (lastColon !== -1 && input.slice(lastColon + 1).includes(".")) {
    const v4 = parseIPv4(input.slice(lastColon + 1));
    if (v4 === null) return null;
    v4Suffix = [(v4 >>> 16) & 0xffff, v4 & 0xffff];
    input = input.slice(0, lastColon + 1) + "0:0";
  }

  const dbl = input.indexOf("::");
  let head = [];
  let tail = [];
  if (dbl !== -1) {
    if (input.indexOf("::", dbl + 2) !== -1) return null;
    const left = input.slice(0, dbl);
    const right = input.slice(dbl + 2);
    head = left === "" ? [] : left.split(":");
    tail = right === "" ? [] : right.split(":");
    if (head.length + tail.length > 7) return null;
  } else {
    head = input.split(":");
    if (head.length !== 8) return null;
  }

  const groups = new Uint16Array(8);
  const parseGroup = (s) => {
    if (!/^[0-9a-fA-F]{1,4}$/.test(s)) return -1;
    return parseInt(s, 16);
  };
  for (let i = 0; i < head.length; i++) {
    const g = parseGroup(head[i]);
    if (g < 0) return null;
    groups[i] = g;
  }
  const tailStart = 8 - tail.length;
  for (let i = 0; i < tail.length; i++) {
    const g = parseGroup(tail[i]);
    if (g < 0) return null;
    groups[tailStart + i] = g;
  }
  if (v4Suffix !== null) {
    groups[6] = v4Suffix[0];
    groups[7] = v4Suffix[1];
  }

  const hi =
    (BigInt(groups[0]) << 48n) |
    (BigInt(groups[1]) << 32n) |
    (BigInt(groups[2]) << 16n) |
    BigInt(groups[3]);
  const lo =
    (BigInt(groups[4]) << 48n) |
    (BigInt(groups[5]) << 32n) |
    (BigInt(groups[6]) << 16n) |
    BigInt(groups[7]);
  return (hi << 64n) | lo;
}

// RFC 5952 canonical IPv6 formatting.
export function formatIPv6(val) {
  const groups = new Array(8);
  for (let i = 7; i >= 0; i--) {
    groups[i] = Number(val & 0xffffn);
    val >>= 16n;
  }
  let bestStart = -1;
  let bestLen = 0;
  let curStart = -1;
  let curLen = 0;
  for (let i = 0; i < 8; i++) {
    if (groups[i] === 0) {
      if (curStart === -1) {
        curStart = i;
        curLen = 1;
      } else {
        curLen++;
      }
      if (curLen > bestLen) {
        bestStart = curStart;
        bestLen = curLen;
      }
    } else {
      curStart = -1;
      curLen = 0;
    }
  }
  if (bestLen < 2) bestStart = -1;

  const parts = [];
  for (let i = 0; i < 8; i++) {
    if (i === bestStart) {
      parts.push("");
      if (i === 0) parts.push("");
      i += bestLen - 1;
      if (i === 7) parts.push("");
    } else {
      parts.push(groups[i].toString(16));
    }
  }
  return parts.join(":");
}

const V4_MAPPED_PREFIX = 0xffff00000000n;
const V4_MAPPED_MASK = ~0xffffffffn;

export function parseIP(raw) {
  const s = (raw || "").trim();
  if (!s) return null;
  const v4 = parseIPv4(s);
  if (v4 !== null) {
    return { isV6: false, value: v4, canonical: formatIPv4(v4) };
  }
  const v6 = parseIPv6(s);
  if (v6 === null) return null;
  if ((v6 & V4_MAPPED_MASK) === V4_MAPPED_PREFIX) {
    const unmapped = Number(v6 & 0xffffffffn) >>> 0;
    return { isV6: false, value: unmapped, canonical: formatIPv4(unmapped) };
  }
  return { isV6: true, value: v6, canonical: formatIPv6(v6) };
}

export function maskIPv4Range(addr, bits) {
  if (bits === 0) return { start: 0, end: 0xffffffff >>> 0, cidr: "0.0.0.0/0" };
  const mask = (~0 << (32 - bits)) >>> 0;
  const start = (addr & mask) >>> 0;
  const end = (start | (~mask >>> 0)) >>> 0;
  return { start, end, cidr: `${formatIPv4(start)}/${bits}` };
}

export function maskIPv6Range(addr, bits) {
  if (bits === 0) {
    return { start: 0n, end: (1n << 128n) - 1n, cidr: "::/0" };
  }
  const shift = BigInt(128 - bits);
  const start = (addr >> shift) << shift;
  const end = start | ((1n << shift) - 1n);
  return { start, end, cidr: `${formatIPv6(start)}/${bits}` };
}

export function parsePrefix(raw) {
  const s = (raw || "").trim();
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
    return {
      isV6: false,
      version: 4,
      bits,
      cidr,
      start,
      end,
      startIP: formatIPv4(start),
      endIP: formatIPv4(end),
      addressCount: (1n << BigInt(32 - bits)).toString(),
    };
  }
  const v6 = parseIPv6(ipStr);
  if (v6 === null || bits < 0 || bits > 128) return null;
  if ((v6 & V4_MAPPED_MASK) === V4_MAPPED_PREFIX && bits >= 96) {
    const mappedBits = bits - 96;
    const mappedV4 = Number(v6 & 0xffffffffn) >>> 0;
    const { start, end, cidr } = maskIPv4Range(mappedV4, mappedBits);
    return {
      isV6: false,
      version: 4,
      bits: mappedBits,
      cidr,
      start,
      end,
      startIP: formatIPv4(start),
      endIP: formatIPv4(end),
      addressCount: (1n << BigInt(32 - mappedBits)).toString(),
    };
  }
  const { start, end, cidr } = maskIPv6Range(v6, bits);
  return {
    isV6: true,
    version: 6,
    bits,
    cidr,
    start,
    end,
    startIP: formatIPv6(start),
    endIP: formatIPv6(end),
    addressCount: (1n << BigInt(128 - bits)).toString(),
  };
}
