import { createSignal } from "solid-js";
import { isIP, isIPv4, isIPv6 } from "./ip.ts";
import type {
  ApiPayload,
  AsnResponse,
  LookupQuery,
  LookupResponse,
  PrefixResponse,
  RangeResponse
} from "../types/api.ts";

export const FULL_API_BASE = "https://bgp-api.mehrnet.com";
export const LITE_API_BASE = "https://bgp-lite.mehrnet.com";
const FALLBACK_KEY = "bgp_api_fallback_until";
const FALLBACK_TTL_MS = 30 * 60 * 1000;
const HEALTH_TIMEOUT_MS = 3500;
const DETECT_TIMEOUT_MS = 4500;
const RANGE_LITE_MSG = "Range scans require the full BGP API, which is currently unavailable. IP, prefix, and ASN lookups continue on the Lite API.";

const [apiModeSignal, setApiModeSignal] = createSignal<"full" | "lite">("full");
export const apiMode = apiModeSignal;
let probePromise: Promise<"full" | "lite"> | null = null;

function readFallbackUntil(): number {
  try { return Number(localStorage.getItem(FALLBACK_KEY) ?? "0") || 0; } catch { return 0; }
}

export function markLiteFallback(): void {
  setApiModeSignal("lite");
  try { localStorage.setItem(FALLBACK_KEY, String(Date.now() + FALLBACK_TTL_MS)); } catch { /* ignore */ }
}

async function probeFullApi(): Promise<"full" | "lite"> {
  try {
    const res = await fetch(`${FULL_API_BASE}/v1/health`, {
      signal: AbortSignal.timeout(HEALTH_TIMEOUT_MS),
      cache: "no-store"
    });
    const health = (await res.json().catch(() => null)) as { ok?: boolean; mode?: string; database?: string } | null;
    if (res.ok && health?.ok === true) {
      const mode = health.mode === "lite" || health.database === "lite" ? "lite" : "full";
      setApiModeSignal(mode);
      return mode;
    }
  } catch { /* fall through to lite */ }
  markLiteFallback();
  return "lite";
}

export async function resolveApiBase(): Promise<string> {
  if (readFallbackUntil() > Date.now()) { setApiModeSignal("lite"); return LITE_API_BASE; }
  if (!probePromise) probePromise = probeFullApi().finally(() => { probePromise = null; });
  return (await probePromise) === "full" ? FULL_API_BASE : LITE_API_BASE;
}

export function buildEndpointPath(query: LookupQuery): string {
  if (query.kind === "ip" && query.value) return `/v1/ip?query=${encodeURIComponent(query.value)}`;
  if (query.kind === "cidr" && query.value) {
    const p = new URLSearchParams({ prefix: query.value });
    if (query.cursor) p.set("cursor", query.cursor);
    return `/v1/prefix?${p.toString()}`;
  }
  if (query.kind === "asn" && query.value) {
    const p = new URLSearchParams({ query: query.value, limit: String(query.limit ?? 50) });
    if (query.page && query.page > 1) p.set("page", String(query.page));
    else if (query.cursor) p.set("cursor", query.cursor);
    else p.set("page", "1");
    return `/v1/asn?${p.toString()}`;
  }
  if (query.kind === "range") {
    const p = new URLSearchParams({ start: query.start ?? "", end: query.end ?? "", kind: query.rangeKind ?? "allocations" });
    if (query.cursor) p.set("cursor", query.cursor);
    return `/v1/range?${p.toString()}`;
  }
  throw new Error("Unsupported lookup query");
}

async function fetchJson<T>(url: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(url, signal ? { signal } : undefined);
  const body = (await res.json().catch(() => null)) as (T & { error?: { message?: string } }) | null;
  if (!res.ok) throw new Error(body?.error?.message ?? `Request failed (${res.status})`);
  if (!body) throw new Error("Empty response from API");
  return body;
}

export async function fetchLookup(query: LookupQuery, signal?: AbortSignal): Promise<ApiPayload> {
  const path = buildEndpointPath(query);
  const base = await resolveApiBase();
  if (query.kind === "range" && apiMode() === "lite") throw new Error(RANGE_LITE_MSG);
  let raw: unknown;
  try {
    raw = await fetchJson<unknown>(`${base}${path}`, signal);
  } catch (err) {
    if (signal?.aborted || base === LITE_API_BASE) throw err;
    markLiteFallback();
    if (query.kind === "range") throw new Error(RANGE_LITE_MSG);
    raw = await fetchJson<unknown>(`${LITE_API_BASE}${path}`, signal);
  }
  if (query.kind === "ip") return { kind: "ip", data: raw as LookupResponse };
  if (query.kind === "cidr") return { kind: "cidr", data: raw as PrefixResponse };
  if (query.kind === "asn") return { kind: "asn", data: raw as AsnResponse };
  return { kind: "range", data: raw as RangeResponse };
}

async function fetchTraceIp(): Promise<string> {
  const res = await fetch("/cdn-cgi/trace", { signal: AbortSignal.timeout(DETECT_TIMEOUT_MS), cache: "no-store" });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const match = /^ip=([^\s]+)$/m.exec(await res.text());
  if (match?.[1] && isIP(match[1])) return match[1].trim();
  throw new Error("Trace IP missing");
}

async function fetchIpify(url: string): Promise<string> {
  const res = await fetch(url, { signal: AbortSignal.timeout(DETECT_TIMEOUT_MS), cache: "no-store" });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const data = (await res.json()) as { ip?: string };
  if (data.ip && isIP(data.ip)) return data.ip.trim();
  throw new Error("Invalid ipify response");
}

export async function detectVisitorIps(): Promise<{ primary: string; ipv6: string | null }> {
  const [traceRes, v4Res] = await Promise.allSettled([fetchTraceIp(), fetchIpify("https://api.ipify.org?format=json")]);
  const traceIp = traceRes.status === "fulfilled" ? traceRes.value : null;
  const v4Ip = v4Res.status === "fulfilled" && isIPv4(v4Res.value) ? v4Res.value : null;
  if (v4Ip) return { primary: v4Ip, ipv6: traceIp && isIPv6(traceIp) ? traceIp : null };
  if (traceIp) return { primary: traceIp, ipv6: null };
  throw new Error("Unable to detect your public IP address automatically.");
}

export function detectOptionalIpv6(): Promise<string> {
  return fetchIpify("https://api6.ipify.org?format=json").then((ip) => {
    if (!isIPv6(ip)) throw new Error("Not IPv6");
    return ip;
  });
}
