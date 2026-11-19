import { For, Show, type JSX } from "solid-js";
import { Icon } from "./Icon.tsx";

export interface ObjectListItem {
  title: JSX.Element | string;
  subtitle?: string | null;
}

interface ObjectListPanelProps {
  title: string;
  icon: string;
  count?: number;
  items: ObjectListItem[];
  emptyText?: string;
}

export function ObjectListPanel(props: ObjectListPanelProps): JSX.Element {
  return (
    <section class="record-panel">
      <header class="record-panel-header">
        <h3 class="record-heading">
          <Icon id={props.icon} />
          <span>{props.title}</span>
        </h3>
        <Show when={typeof props.count === "number"}>
          <span class="object-count">{props.count}</span>
        </Show>
      </header>
      <Show
        when={props.items.length > 0}
        fallback={
          <div class="object-list">
            <div class="object-row">
              <span>{props.emptyText ?? "No records found."}</span>
            </div>
          </div>
        }
      >
        <div class="object-list">
          <For each={props.items}>
            {(item) => (
              <div class="object-row">
                <strong>{item.title}</strong>
                <Show when={item.subtitle}>
                  {(sub) => <span>{sub()}</span>}
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>
    </section>
  );
}
