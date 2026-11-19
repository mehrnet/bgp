import type { LookupQuery } from "../types/api.ts";

export interface RouteState {
  view: "home" | "api";
  apiSection: string | null;
  isMyIp: boolean;
  query: LookupQuery | null;
}

const API_SECTIONS = new Set(["ip", "prefix", "range", "asn"]);

export function queryToPath(query: LookupQuery, isMyIp = false): string {
  if (isMyIp) return "/my-ip";
  if (query.kind === "ip" && query.value) {
    return `/ip/${encodeURIComponent(query.value)}`;
  }
  if (query.kind === "cidr" && query.value) {
    return `/cidr/${query.value.split("/").map(encodeURIComponent).join("/")}`;
  }
  if (query.kind === "asn" && query.value) {
    const params = new URLSearchParams();
    if (query.page && query.page > 1) params.set("page", String(query.page));
    else if (query.cursor) params.set("cursor", query.cursor);
    const qs = params.toString();
    return `/asn/${encodeURIComponent(query.value)}${qs ? `?${qs}` : ""}`;
  }
  if (query.kind === "range") {
    const params = new URLSearchParams();
    if (query.start) params.set("start", query.start);
    if (query.end) params.set("end", query.end);
    if (query.rangeKind) params.set("kind", query.rangeKind);
    return `/range?${params.toString()}`;
  }
  return "/my-ip";
}

function parseAsnRoute(segments: string[], search: URLSearchParams): RouteState | null {
  const value = segments[1];
  if (!value) return null;
  const pageRaw = Number(search.get("page") ?? "");
  const page = Number.isInteger(pageRaw) && pageRaw > 0 ? pageRaw : undefined;
  const cursor = search.get("cursor") ?? undefined;
  const query: LookupQuery = {
    kind: "asn",
    value,
    ...(page ? { page } : {}),
    ...(cursor && !page ? { cursor } : {})
  };
  return { view: "home", apiSection: null, isMyIp: false, query };
}

function parseRangeRoute(search: URLSearchParams): RouteState | null {
  const start = search.get("start") ?? "";
  const end = search.get("end") ?? "";
  if (!start || !end) return null;
  const rangeKind = search.get("kind") === "routes" ? "routes" : "allocations";
  return {
    view: "home",
    apiSection: null,
    isMyIp: false,
    query: { kind: "range", start, end, rangeKind }
  };
}

function parseLookupSegments(segments: string[], search: URLSearchParams): RouteState | null {
  const [head, arg] = segments;
  if (head === "ip" && arg) {
    return { view: "home", apiSection: null, isMyIp: false, query: { kind: "ip", value: arg } };
  }
  if (head === "cidr" && segments.length >= 3) {
    return {
      view: "home",
      apiSection: null,
      isMyIp: false,
      query: { kind: "cidr", value: segments.slice(1).join("/") }
    };
  }
  if (head === "asn") return parseAsnRoute(segments, search);
  if (head === "range") return parseRangeRoute(search);
  return null;
}

export function parseRoute(pathname: string, searchStr: string): RouteState {
  const clean = pathname.replace(/\/+$/, "") || "/";
  const search = new URLSearchParams(searchStr);

  if (clean === "/api" || clean.startsWith("/api/")) {
    const sub = clean.slice(5);
    return {
      view: "api",
      apiSection: API_SECTIONS.has(sub) ? `api-${sub}` : null,
      isMyIp: false,
      query: null
    };
  }
  if (clean === "/" || clean === "/home" || clean === "/my-ip") {
    return { view: "home", apiSection: null, isMyIp: true, query: null };
  }
  const segments = clean.slice(1).split("/").map((s) => decodeURIComponent(s));
  return parseLookupSegments(segments, search) ?? {
    view: "home",
    apiSection: null,
    isMyIp: true,
    query: null
  };
}
