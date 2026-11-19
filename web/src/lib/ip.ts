import type { LookupNetwork, LookupQuery, RoutePage } from "../types/api.ts";

const IPV4_PART = "(?:25[0-5]|2[0-4]\\d|1\\d\\d|[1-9]?\\d)";
const IPV4_REGEX = new RegExp(`^(?:${IPV4_PART}\\.){3}${IPV4_PART}$`);

export function isIPv4(value: string): boolean {
  return IPV4_REGEX.test(value.trim());
}

export function isIPv6(value: string): boolean {
  const trimmed = value.trim();
  if (!trimmed || !trimmed.includes(":")) return false;
  if (trimmed.includes(":::") || trimmed.split("::").length > 2) return false;
  if (trimmed.includes(".")) {
    const lastColon = trimmed.lastIndexOf(":");
    const v4Part = trimmed.slice(lastColon + 1);
    if (!isIPv4(v4Part)) return false;
  }
  const halves = trimmed.split("::");
  const head = halves[0] ? halves[0].split(":") : [];
  const tail = halves[1] ? halves[1].split(":") : [];
  const hasV4Tail = trimmed.includes(".");
  const checkGroups = (groups: string[], skipLast: boolean): boolean =>
    groups.every((g, idx) =>
      (skipLast && idx === groups.length - 1) || /^[0-9a-fA-F]{1,4}$/.test(g)
    );
  if (!checkGroups(head, hasV4Tail && halves.length === 1)) return false;
  if (!checkGroups(tail, hasV4Tail && halves.length === 2)) return false;
  const totalGroups = head.length + tail.length + (hasV4Tail ? 1 : 0);
  return halves.length === 2 ? totalGroups < 8 : totalGroups === 8;
}

export function isIP(value: string): boolean {
  return isIPv4(value) || isIPv6(value);
}

export function parseCidr(value: string): string | null {
  const parts = value.trim().split("/");
  if (parts.length !== 2) return null;
  const [ip, prefixRaw] = parts;
  if (!ip || !prefixRaw || !/^\d+$/.test(prefixRaw)) return null;
  const prefix = Number(prefixRaw);
  if (isIPv4(ip) && prefix >= 0 && prefix <= 32) return `${ip}/${prefix}`;
  if (isIPv6(ip) && prefix >= 0 && prefix <= 128) return `${ip}/${prefix}`;
  return null;
}

export function parseAsn(value: string): string | null {
  const match = /^(?:AS)?(\d+)$/i.exec(value.trim());
  if (!match?.[1]) return null;
  const num = Number(match[1]);
  if (!Number.isSafeInteger(num) || num < 0 || num > 0xffffffff) return null;
  return `AS${num}`;
}

export function classifyQuery(raw: string): LookupQuery | null {
  const value = raw.trim();
  if (!value) return null;

  const rangeMatch = /^(.+?)\s+-\s+(.+)$/.exec(value);
  if (rangeMatch?.[1] && rangeMatch[2]) {
    const start = rangeMatch[1].trim();
    const end = rangeMatch[2].trim();
    if ((isIPv4(start) && isIPv4(end)) || (isIPv6(start) && isIPv6(end))) {
      return { kind: "range", start, end, rangeKind: "allocations" };
    }
  }

  const cidr = parseCidr(value);
  if (cidr) return { kind: "cidr", value: cidr };
  if (isIP(value)) return { kind: "ip", value };

  const asn = parseAsn(value);
  if (asn) return { kind: "asn", value: asn };

  return null;
}

export function normalizeQueryLabel(query: LookupQuery): string {
  if (query.kind === "range") {
    return `${query.start ?? ""} - ${query.end ?? ""}`.trim();
  }
  return query.value ?? "";
}

export function parseCoord(value: unknown): number | null {
  if (value === null || value === undefined || value === "") return null;
  const num = typeof value === "number" ? value : Number(value);
  return Number.isFinite(num) ? num : null;
}

export function formatCoord(value: number): string {
  return value.toFixed(4).replace(/\.?0+$/, "");
}

export function formatRange(start?: string | null, end?: string | null): string {
  if (start && end) return `${start} - ${end}`;
  return start ?? end ?? "Unavailable";
}

export function networkAsns(network?: LookupNetwork): string[] {
  if (!network) return [];
  const seen = new Set<string>();
  const add = (val?: string | null) => {
    const trimmed = val?.trim();
    if (trimmed) seen.add(trimmed.toUpperCase());
  };
  network.origins?.forEach((o) => add(o.asn));
  network.asns?.forEach((a) => add(a));
  add(network.asn);
  return Array.from(seen);
}

export function buildPaginationTokens(
  current: number,
  total: number
): (number | "ellipsis")[] {
  if (total <= 1) return [1];
  const pages = new Set<number>([1, total]);
  for (let p = current - 2; p <= current + 2; p += 1) {
    if (p > 1 && p < total) pages.add(p);
  }
  const sorted = Array.from(pages).sort((a, b) => a - b);
  const tokens: (number | "ellipsis")[] = [];
  let prev = 0;
  for (const page of sorted) {
    if (prev && page - prev > 1) tokens.push("ellipsis");
    tokens.push(page);
    prev = page;
  }
  return tokens;
}

export function formatPageSummary(routes: RoutePage): string {
  const count = routes.items.length;
  const page = routes.page ?? 1;
  const totalPages = routes.total_pages ?? 1;
  const totalItems = routes.total_items;
  if (totalItems && totalPages > 1) {
    return `Page ${page} of ${totalPages} (${totalItems} routes)`;
  }
  return `${count} ${count === 1 ? "route" : "routes"}`;
}
