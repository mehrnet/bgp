import { Show, type JSX } from "solid-js";
import { apiMode } from "../../lib/apiClient.ts";
import { Icon } from "../ui/Icon.tsx";

interface NavHeaderProps {
  currentView: "home" | "api";
  onOpenHome: (event: MouseEvent) => void;
  onOpenApi: (event: MouseEvent) => void;
}

export function NavHeader(props: NavHeaderProps): JSX.Element {
  return (
    <header class="nav">
      <div class="nav-inner">
        <a class="brand" href="/my-ip" onClick={props.onOpenHome}>
          <Icon id="icon-bgp-mark" class="brand-mark" />
          <span class="brand-copy">
            <span class="brand-name">
              Mehrnet <span>BGP</span>
            </span>
            <span class="brand-byline">Routing &amp; RIR explorer</span>
          </span>
        </a>
        <div class="nav-actions">
          <Show when={apiMode() === "lite"}>
            <span
              class="api-mode-badge"
              title="Full API unreachable; using bgp-lite edge fallback"
            >
              Lite fallback
            </span>
          </Show>
          <a
            class="api-link"
            href="/my-ip"
            aria-current={props.currentView === "home" ? "page" : undefined}
            onClick={props.onOpenHome}
          >
            <Icon id="icon-search" />
            <span>Explorer</span>
          </a>
          <a
            class="api-link"
            href="/api"
            aria-current={props.currentView === "api" ? "page" : undefined}
            onClick={props.onOpenApi}
          >
            <Icon id="icon-terminal" />
            <span>API Docs</span>
          </a>
        </div>
      </div>
    </header>
  );
}
