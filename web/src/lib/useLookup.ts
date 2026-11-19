import { createSignal } from "solid-js";
import type { ApiPayload, LookupQuery } from "../types/api.ts";
import { detectOptionalIpv6, detectVisitorIps, fetchLookup } from "./apiClient.ts";
import { normalizeQueryLabel } from "./ip.ts";

interface TaskRefs {
  setInput: (v: string) => void;
  setLoading: (v: boolean) => void;
  setError: (v: string | null) => void;
  setPayload: (v: ApiPayload | null) => void;
}

async function runQueryTask(query: LookupQuery, isMyIp: boolean, signal: AbortSignal, refs: TaskRefs) {
  refs.setLoading(true);
  refs.setError(null);
  refs.setInput(normalizeQueryLabel(query));
  document.title = isMyIp ? "MehrBGP — My IP" : `${normalizeQueryLabel(query)} — MehrBGP`;
  try {
    const result = await fetchLookup(query, signal);
    if (!signal.aborted) refs.setPayload(result);
  } catch (err) {
    if (!signal.aborted) {
      refs.setError(err instanceof Error ? err.message : "Lookup failed");
      refs.setPayload(null);
    }
  } finally {
    if (!signal.aborted) refs.setLoading(false);
  }
}

export function createLookupState() {
  const [inputValue, setInputValue] = createSignal("");
  const [loading, setLoading] = createSignal(false);
  const [lookupError, setLookupError] = createSignal<string | null>(null);
  const [payload, setPayload] = createSignal<ApiPayload | null>(null);
  const [ipv6, setIpv6] = createSignal<string | null>(null);
  let activeAbort: AbortController | null = null;
  let ipv6Checked = false;
  const refs: TaskRefs = { setInput: setInputValue, setLoading, setError: setLookupError, setPayload };

  const executeLookup = async (query: LookupQuery, isMyIp: boolean) => {
    activeAbort?.abort();
    activeAbort = new AbortController();
    await runQueryTask(query, isMyIp, activeAbort.signal, refs);
  };

  const detectPublicIp = async () => {
    setLoading(true);
    setLookupError(null);
    try {
      const { primary, ipv6: traceV6 } = await detectVisitorIps();
      if (traceV6) { setIpv6(traceV6); ipv6Checked = true; }
      else if (!ipv6Checked) { ipv6Checked = true; void detectOptionalIpv6().then(setIpv6).catch(() => undefined); }
      await executeLookup({ kind: "ip", value: primary }, true);
    } catch {
      setLoading(false);
      setLookupError("Unable to detect your public IP address automatically.");
    }
  };

  return { inputValue, setInputValue, loading, lookupError, payload, ipv6, executeLookup, detectPublicIp, abort: () => activeAbort?.abort() };
}
