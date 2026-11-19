import { createSignal, For, onMount, type JSX } from "solid-js";
import {
  ENDPOINT_SPECS,
  LANGUAGE_OPTIONS,
  TIER_COMPARISON,
  type DocLanguage
} from "../../data/apiDocs.ts";
import { Icon } from "../ui/Icon.tsx";
import { EndpointDoc } from "./EndpointDoc.tsx";

const LANG_KEY = "bgp_api_language";

function readStoredLanguage(): DocLanguage {
  try {
    const raw = localStorage.getItem(LANG_KEY);
    if (LANGUAGE_OPTIONS.some((opt) => opt.id === raw)) return raw as DocLanguage;
  } catch {
    // Ignore storage errors.
  }
  return "curl";
}

function EndpointDirectory(): JSX.Element {
  return (
    <section class="api-section">
      <div class="api-section-heading">
        <p class="kicker">Endpoints</p>
        <h2>Available resources</h2>
      </div>
      <div class="api-endpoints">
        <For each={ENDPOINT_SPECS}>
          {(spec) => (
            <a href={`#${spec.id}`}>
              <span class="endpoint-method">{spec.method}</span>
              <code>{spec.pathTemplate}</code>
              <span>{spec.summary}</span>
            </a>
          )}
        </For>
      </div>
      <p class="api-note">
        The UI probes <code>/v1/health</code> on the Full API and fails over to{" "}
        <code>https://bgp-lite.mehrnet.com</code> for 30 minutes if unreachable.
      </p>
    </section>
  );
}

function TierTable(): JSX.Element {
  return (
    <section class="api-section">
      <div class="api-section-heading">
        <p class="kicker">Tiers</p>
        <h2>Full API vs Lite Worker</h2>
      </div>
      <div class="api-example">
        <div class="api-parameters api-parameters-flush">
          <div class="api-table-scroll" tabindex="0">
            <table>
              <thead>
                <tr><th>Capability</th><th>Full API</th><th>Lite Worker</th></tr>
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
    </section>
  );
}

export function ApiDocsView(props: { activeSection: string | null; onCopied: (msg: string) => void }): JSX.Element {
  const [language, setLanguage] = createSignal<DocLanguage>(readStoredLanguage());
  const selectLanguage = (lang: DocLanguage) => {
    setLanguage(lang);
    try { localStorage.setItem(LANG_KEY, lang); } catch { /* ignore */ }
  };

  onMount(() => {
    if (props.activeSection) {
      document.getElementById(props.activeSection)?.scrollIntoView({ behavior: "smooth", block: "start" });
    }
  });

  return (
    <div class="api-view">
      <div class="api-overview">
        <span class="section-label"><Icon id="icon-terminal" /><span>Base URL</span></span>
        <code>https://bgp-api.mehrnet.com</code>
        <span>Unauthenticated read-only JSON API backed by RIR &amp; RouteViews data.</span>
        <a class="openapi-link" href="/openapi.json" target="_blank" rel="noopener noreferrer">
          <Icon id="icon-book-open" /><span>OpenAPI spec</span>
        </a>
      </div>
      <EndpointDirectory />
      <TierTable />
      <section class="api-section">
        <div class="api-section-heading"><p class="kicker">Reference</p><h2>Interactive examples</h2></div>
        <div class="api-language-bar">
          <span>Language</span>
          <div class="language-tabs" role="tablist" aria-label="Code language">
            <For each={LANGUAGE_OPTIONS}>
              {(opt) => (
                <button type="button" class="language-tab" aria-pressed={language() === opt.id} onClick={() => selectLanguage(opt.id)}>
                  <Icon id={opt.icon} /><span>{opt.label}</span>
                </button>
              )}
            </For>
          </div>
        </div>
        <div class="api-examples">
          <For each={ENDPOINT_SPECS}>{(spec) => <EndpointDoc spec={spec} language={language()} onCopied={props.onCopied} />}</For>
        </div>
      </section>
    </div>
  );
}
