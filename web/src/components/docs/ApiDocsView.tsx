import { createSignal, For, onMount, type JSX } from "solid-js";
import {
  DIRECTORY_LINKS,
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
  } catch { /* ignore */ }
  return "curl";
}

function EndpointDirectory(): JSX.Element {
  return (
    <section class="api-section">
      <div class="api-section-heading">
        <p class="kicker">Endpoints</p>
        <h2>Lookup resources</h2>
      </div>
      <div class="api-endpoints">
        <For each={DIRECTORY_LINKS}>
          {(item) => (
            <a href={item.href}>
              <span class="endpoint-method">{item.method}</span>
              <code>{item.path}</code>
              <span>{item.note}</span>
            </a>
          )}
        </For>
      </div>
      <p class="api-note">
        Both <code>bgp-api.mehrnet.com</code> (Full) and <code>bgp-lite.mehrnet.com</code> (Edge Lite) share compatible lookup schemas. When Full API health fails, this UI automatically fails over to Lite.
      </p>
    </section>
  );
}

function TierTable(): JSX.Element {
  return (
    <section class="api-section">
      <div class="api-section-heading">
        <p class="kicker">Endpoints comparison</p>
        <h2>Full API vs. Lite API</h2>
      </div>
      <div class="api-example">
        <div class="api-parameters api-parameters-flush">
          <div class="api-table-scroll" tabindex="0">
            <table>
              <thead>
                <tr>
                  <th>Capability</th>
                  <th><code>bgp-api.mehrnet.com</code> (Full)</th>
                  <th><code>bgp-lite.mehrnet.com</code> (Lite)</th>
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
      <div id="api-overview" class="api-overview">
        <span class="section-label"><Icon id="icon-globe" /><span>Base URLs</span></span>
        <code>https://bgp-api.mehrnet.com · https://bgp-lite.mehrnet.com</code>
        <span>No key, no client library. Use Full API for WHOIS &amp; range scans, or Lite Worker for edge lookups &amp; <code>/v1/country</code>.</span>
        <a class="openapi-link" href="/openapi.json" target="_blank" rel="noopener noreferrer">
          <Icon id="icon-book-open" /><span>OpenAPI JSON</span>
        </a>
      </div>
      <TierTable />
      <EndpointDirectory />
      <section class="api-section">
        <div class="api-section-heading"><p class="kicker">Services</p><h2>Interactive examples</h2></div>
        <div class="api-language-bar">
          <span>Example language</span>
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
