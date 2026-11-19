import { createSignal } from "solid-js";
import { isIP } from "./ip.ts";
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

export const IPV4_PROVIDERS = [
  "https://ipv4.mehrnet.com/",
  "https://api.ipify.org?format=json"
];

export const IPV6_PROVIDERS = [
  "https://ipv6.mehrnet.com/",
  "https://api6.ipify.org?format=json"
];

const [apiModeSignal, setApiModeSignal] = createSignal<"full" | "lite">("full");
export const apiMode = apiModeSignal;

let probePromise: Promise<"full" | "lite"> | null = null;

function readFallbackUntil(): number {
  try {
    return Number(localStorage.getItem(FALLBACK_KEY) ?? "0") || 0;
  } catch {
    return 0;
  }
}

export function markLiteFallback(): void {
  setApiModeSignal("lite");
  try {
    localStorage.setItem(FALLBACK_KEY, String(Date.now() + FALLBACK_TTL_MS));
  } catch {
    // Ignore storage quota / privacy mode failures.
  }
}

export async function resolveApiBase(): Promise<string> {
  if (readFallbackUntil() > Date.now()) {
    setApiModeSignal("lite");
    return LITE_API_BASE;
  }
  if (!probePromise) {
    probePromise = probeFullApi().finally(() => {
      probePromise = null;
    });
  }
  const mode = await probePromise;
  return mode === "full" ? FULL_API_BASE : LITE_API_BASE;
}

async function probeFullApi(): Promise<"full" | "lite"> {
  try {
    const res = await fetch(`${FULL_API_BASE}/v1/health`, {
      signal: AbortSignal.timeout(HEALTH_TIMEOUT_MS)
    });
    if (res.ok) {
      setApiModeSignal("full");
      return "full";
    }
  } catch {
    // Fall through to lite fallback.
  }
  markLiteFallback();
  return "lite";
}

export function buildEndpointPath(query: LookupQuery): string {
  if (query.kind === "ip" && query.value) {
    return `/v1/ip/${encodeURIComponent(query.value)}?details=true`;
  }
  if (query.kind === "cidr" && query.value) {
    const encoded = query.value.split("/").map(encodeURIComponent).join("/");
    return `/v1/prefix/${encoded}`;
  }
  if (query.kind === "asn" && query.value) {
    const params = new URLSearchParams({ limit: String(query.limit ?? 50) });
    if (query.page && query.page > 1) params.set("page", String(query.page));
    else if (query.cursor) params.set("cursor", query.cursor);
    return `/v1/asn/${encodeURIComponent(query.value)}?${params.toString()}`;
  }
  if (query.kind === "range") {
    const params = new URLSearchParams({
      start: query.start ?? "",
      end: query.end ?? "",
      kind: query.rangeKind ?? "allocations"
    });
    if (query.cursor) params.set("cursor", query.cursor);
    return `/v1/range?${params.toString()}`;
  }
  throw new Error("Unsupported lookup query");
}

async function fetchJson<T>(url: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(url, signal ? { signal } : undefined);
  const body = (await res.json().catch(() => null)) as
    | (T & { error?: { message?: string } })
    | null;
  if (!res.ok) {
    throw new Error(body?.error?.message ?? `Request failed (${res.status})`);
  }
  if (!body) throw new Error("Empty response from API");
  return body;
}

export async function fetchLookup(
  query: LookupQuery,
  signal?: AbortSignal
): Promise<ApiPayload> {
  const path = buildEndpointPath(query);
  const base = await resolveApiBase();
  let raw: unknown;

  try {
    raw = await fetchJson<unknown>(`${base}${path}`, signal);
  } catch (err) {
    if (signal?.aborted || base === LITE_API_BASE) throw err;
    markLiteFallback();
    raw = await fetchJson<unknown>(`${LITE_API_BASE}${path}`, signal);
  }

  if (query.kind === "ip") return { kind: "ip", data: raw as LookupResponse };
  if (query.kind === "cidr") return { kind: "cidr", data: raw as PrefixResponse };
  if (query.kind === "asn") return { kind: "asn", data: raw as AsnResponse };
  return { kind: "range", data: raw as RangeResponse };
}

async function fetchAddressFromProvider(url: string): Promise<string> {
  const res = await fetch(url, {
    signal: AbortSignal.timeout(DETECT_TIMEOUT_MS)
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const text = await res.text();
  const trimmed = text.trim();
  if (trimmed.startsWith("{")) {
    const parsed = JSON.parse(trimmed) as { ip?: string };
    if (parsed.ip && isIP(parsed.ip)) return parsed.ip.trim();
  }
  if (isIP(trimmed)) return trimmed;
  throw new Error("Invalid IP response");
}

export function raceAddressProviders(urls: string[]): Promise<string> {
  return Promise.any(urls.map(fetchAddressFromProvider));
}
