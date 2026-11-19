import { For, Show, type JSX } from "solid-js";
import { Icon } from "./Icon.tsx";

export interface RecordField {
  label: string;
  value: JSX.Element | string | number | null | undefined;
}

interface RecordPanelProps {
  title: string;
  icon: string;
  support?: string | null | undefined;
  fields: RecordField[];
  extra?: JSX.Element | undefined;
}

export function RecordPanel(props: RecordPanelProps): JSX.Element {
  const visibleFields = () =>
    props.fields.filter(
      (f) => f.value !== null && f.value !== undefined && f.value !== ""
    );

  return (
    <section class="record-panel">
      <header class="record-panel-header">
        <h3 class="record-heading">
          <Icon id={props.icon} />
          <span>{props.title}</span>
        </h3>
        <Show when={props.support}>
          {(sup) => <span class="panel-support">{sup()}</span>}
        </Show>
      </header>
      <dl class="record-list">
        <For each={visibleFields()}>
          {(field) => (
            <div class="record-row">
              <dt>{field.label}</dt>
              <dd>{field.value}</dd>
            </div>
          )}
        </For>
      </dl>
      {props.extra}
    </section>
  );
}
