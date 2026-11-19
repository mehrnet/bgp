import { For, Show, type JSX } from "solid-js";
import { buildPaginationTokens, formatPageSummary } from "../../lib/ip.ts";
import type { RoutePage } from "../../types/api.ts";
import { Icon } from "../ui/Icon.tsx";

interface PaginationNavProps {
  routes: RoutePage;
  onSelectPage: (page: number) => void;
  onNextCursor: (cursor: string) => void;
}

function StepButton(props: {
  class: string;
  label: string;
  disabled: boolean;
  double?: boolean;
  onClick: () => void;
}): JSX.Element {
  return (
    <button
      type="button"
      class={`pagination-control ${props.class}`}
      aria-label={props.label}
      disabled={props.disabled}
      onClick={props.onClick}
    >
      <Show when={props.double} fallback={<Icon id="icon-arrow-right" />}>
        <span class="pagination-double-icon">
          <Icon id="icon-arrow-right" />
          <Icon id="icon-arrow-right" />
        </span>
      </Show>
    </button>
  );
}

export function PaginationNav(props: PaginationNavProps): JSX.Element {
  const current = () => props.routes.page ?? 1;
  const total = () => props.routes.total_pages ?? 1;

  return (
    <Show
      when={total() > 1}
      fallback={
        <Show when={props.routes.next_cursor}>
          {(cursor) => (
            <button type="button" class="more-button" onClick={() => props.onNextCursor(cursor())}>
              <span>Load more routes</span>
              <Icon id="icon-arrow-right" />
            </button>
          )}
        </Show>
      }
    >
      <nav class="route-pagination" aria-label="Route pagination">
        <span class="pagination-summary">{formatPageSummary(props.routes)}</span>
        <div class="pagination-controls">
          <StepButton class="pagination-control-first" label="First page" disabled={current() <= 1} double onClick={() => props.onSelectPage(1)} />
          <StepButton class="pagination-control-previous" label="Previous page" disabled={current() <= 1} onClick={() => props.onSelectPage(current() - 1)} />
          <For each={buildPaginationTokens(current(), total())}>
            {(token) =>
              token === "ellipsis" ? (
                <span class="pagination-summary">…</span>
              ) : (
                <button
                  type="button"
                  class="pagination-page"
                  classList={{ "is-current": token === current() }}
                  aria-current={token === current() ? "page" : undefined}
                  onClick={() => props.onSelectPage(token)}
                >
                  {token}
                </button>
              )
            }
          </For>
          <StepButton class="" label="Next page" disabled={current() >= total()} onClick={() => props.onSelectPage(current() + 1)} />
          <StepButton class="" label="Last page" disabled={current() >= total()} double onClick={() => props.onSelectPage(total())} />
        </div>
      </nav>
    </Show>
  );
}
