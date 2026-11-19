import { createSignal, For, onCleanup, onMount, Show, type JSX } from "solid-js";
import { TIER_COMPARISON } from "../../data/apiDocs.ts";
import { retryFullApi } from "../../lib/apiClient.ts";
import { Icon } from "../ui/Icon.tsx";

interface LiteFallbackModalProps {
  open: boolean;
  onClose: () => void;
  onOpenApi: (event: MouseEvent) => void;
  onStatus: (msg: string) => void;
}

function ComparisonSection(): JSX.Element {
  return (
    <div class="modal-section">
      <h3>bgp-api (Full) vs. bgp-lite (Edge Lite)</h3>
      <div class="api-parameters api-parameters-flush">
        <div class="api-table-scroll" tabindex="0">
          <table>
            <thead>
              <tr>
                <th>Capability</th>
                <th><code>bgp-api</code> (Full)</th>
                <th><code>bgp-lite</code> (Lite)</th>
              </tr>
            </thead>
            <tbody>
              <For each={TIER_COMPARISON}>
                {(row) => <tr><th>{row.feature}</th><td>{row.full}</td><td>{row.lite}</td></tr>}
              </For>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}

function ModalBody(props: { checking: boolean; onRetry: () => void; onOpenApi: (e: MouseEvent) => void; onClose: () => void }): JSX.Element {
  return (
    <>
      <div class="modal-body">
        <div class="modal-section">
          <h3>Why did this happen?</h3>
          <p>
            The Full API (<code>bgp-api.mehrnet.com</code>) did not pass its health check or failed a request, so MehrBGP automatically switched to the serverless Cloudflare edge fallback (<code>bgp-lite.mehrnet.com</code>) for 30 minutes.
          </p>
        </div>
        <div class="modal-section">
          <h3>Does it affect normal lookups?</h3>
          <p>
            Mostly no. IP lookups, BGP origin ASNs, CIDR prefixes, ASN prefix lists, aut-num details, and geofeed map coordinates keep working cleanly. Only full <code>/v1/range</code> scans and extra WHOIS attributes (<code>netname</code>, allocation dates, <code>?details=full</code>) require the Full API.
          </p>
        </div>
        <ComparisonSection />
      </div>
      <footer class="modal-footer">
        <a class="modal-action-button" href="/api" onClick={(e) => { props.onClose(); props.onOpenApi(e); }}>API docs</a>
        <button type="button" class="modal-action-button" disabled={props.checking} onClick={props.onRetry}>
          {props.checking ? "Checking Full API…" : "Retry Full API"}
        </button>
        <button type="button" class="modal-action-button modal-action-primary" onClick={props.onClose}>Got it</button>
      </footer>
    </>
  );
}

export function LiteFallbackModal(props: LiteFallbackModalProps): JSX.Element {
  const [checking, setChecking] = createSignal(false);

  onMount(() => {
    const onKeyDown = (e: KeyboardEvent) => { if (props.open && e.key === "Escape") props.onClose(); };
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => window.removeEventListener("keydown", onKeyDown));
  });

  const handleRetry = async () => {
    setChecking(true);
    const mode = await retryFullApi();
    setChecking(false);
    props.onStatus(mode === "full" ? "Connected back to Full API (bgp-api)" : "Full API still unreachable; staying on Lite fallback");
    if (mode === "full") props.onClose();
  };

  return (
    <Show when={props.open}>
      <div class="modal-backdrop" onClick={(e) => { if (e.target === e.currentTarget) props.onClose(); }}>
        <section class="modal-dialog" role="dialog" aria-modal="true" aria-labelledby="lite-modal-title">
          <header class="modal-header">
            <h2 id="lite-modal-title" class="modal-title"><Icon id="icon-alert" /><span>Running on Lite API fallback</span></h2>
            <button type="button" class="modal-close" aria-label="Close modal" onClick={props.onClose}>Close</button>
          </header>
          <ModalBody checking={checking()} onRetry={() => void handleRetry()} onOpenApi={props.onOpenApi} onClose={props.onClose} />
        </section>
      </div>
    </Show>
  );
}
