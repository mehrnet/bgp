import { createSignal, onCleanup, onMount, Show, type JSX } from "solid-js";
import { ApiDocsView } from "./components/docs/ApiDocsView.tsx";
import { LiteFallbackModal } from "./components/layout/LiteFallbackModal.tsx";
import { NavHeader } from "./components/layout/NavHeader.tsx";
import { LookupResult } from "./components/lookup/LookupResult.tsx";
import { SearchBar } from "./components/lookup/SearchBar.tsx";
import { classifyQuery } from "./lib/ip.ts";
import { parseRoute, queryToPath, type RouteState } from "./lib/routes.ts";
import { createLookupState } from "./lib/useLookup.ts";
import type { LookupQuery } from "./types/api.ts";

function PageHeading(props: { view: "home" | "api" }): JSX.Element {
  return (
    <header class="page-header" classList={{ "api-header": props.view === "api" }}>
      <p class="kicker">{props.view === "api" ? "Public API / field manual" : "Network intelligence"}</p>
      <h1>{props.view === "api" ? "MehrBGP REST API" : "Network lookup"}</h1>
      <Show when={props.view === "api"}>
        <p>Two compatible endpoints for address, prefix, range, and ASN intelligence. JSON over HTTPS, built from MehrNet's routing dataset.</p>
      </Show>
    </header>
  );
}

function useRouter(lookup: ReturnType<typeof createLookupState>) {
  const [route, setRoute] = createSignal<RouteState>(parseRoute(window.location.pathname, window.location.search));
  const [inputError, setInputError] = createSignal<string | null>(null);

  const applyRoute = (next: RouteState) => {
    setRoute(next);
    if (next.view === "api") { document.title = "MehrBGP API Documentation"; return; }
    if (next.isMyIp || !next.query) {
      if (["/", "/home"].includes(window.location.pathname)) window.history.replaceState(null, "", "/my-ip");
      void lookup.detectPublicIp();
    } else void lookup.executeLookup(next.query, false);
  };

  const navigate = (q: LookupQuery) => {
    window.history.pushState(null, "", queryToPath(q));
    setInputError(null);
    applyRoute({ view: "home", apiSection: null, isMyIp: false, query: q });
  };

  const openRoute = (view: "home" | "api", e?: MouseEvent) => {
    if (e && (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey)) return;
    e?.preventDefault();
    window.history.pushState(null, "", view === "api" ? "/api" : "/my-ip");
    applyRoute({ view, apiSection: null, isMyIp: view === "home", query: null });
  };

  return { route, inputError, setInputError, applyRoute, navigate, openRoute };
}

function createToast() {
  const [toast, setToast] = createSignal<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  return { toast, showToast: (msg: string) => { clearTimeout(timer); setToast(msg); timer = setTimeout(() => setToast(null), 2400); } };
}

export function App(): JSX.Element {
  const [liteModalOpen, setLiteModalOpen] = createSignal(false);
  const lookup = createLookupState();
  const router = useRouter(lookup);
  const { toast, showToast } = createToast();

  onMount(() => {
    const onPop = () => router.applyRoute(parseRoute(window.location.pathname, window.location.search));
    window.addEventListener("popstate", onPop);
    onCleanup(() => { window.removeEventListener("popstate", onPop); lookup.abort(); });
    router.applyRoute(router.route());
  });

  const submitSearch = () => {
    const q = classifyQuery(lookup.inputValue());
    if (q) router.navigate(q);
    else router.setInputError("Enter an IP address, CIDR, range, or AS number.");
  };

  return (
    <>
      <NavHeader currentView={router.route().view} onOpenHome={(e) => router.openRoute("home", e)} onOpenApi={(e) => router.openRoute("api", e)} onOpenLiteInfo={() => setLiteModalOpen(true)} />
      <main class="page">
        <PageHeading view={router.route().view} />
        <Show when={router.route().view === "home"} fallback={<ApiDocsView activeSection={router.route().apiSection} onCopied={showToast} />}>
          <SearchBar
            value={lookup.inputValue()}
            onInput={(v) => { lookup.setInputValue(v); router.setInputError(null); }}
            onSubmit={submitSearch}
            onMyIp={() => router.openRoute("home")}
            error={router.inputError()}
            ipv6Candidate={lookup.ipv6()}
            onSelectIpv6={(ip) => router.navigate({ kind: "ip", value: ip })}
          />
          <LookupResult loading={lookup.loading()} error={lookup.lookupError()} payload={lookup.payload()} isMyIp={router.route().isMyIp} onNavigate={router.navigate} onCopied={showToast} />
        </Show>
      </main>
      <LiteFallbackModal open={liteModalOpen()} onClose={() => setLiteModalOpen(false)} onOpenApi={(e) => router.openRoute("api", e)} onStatus={showToast} />
      <Show when={toast()}>{(msg) => <div class="toast" role="status">{msg()}</div>}</Show>
    </>
  );
}
