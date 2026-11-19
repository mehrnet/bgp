import { Match, Show, Switch, type JSX } from "solid-js";
import type { ApiPayload, LookupQuery } from "../../types/api.ts";
import { Icon } from "../ui/Icon.tsx";
import { AsnResultView } from "./AsnResultView.tsx";
import { IpResultView } from "./IpResultView.tsx";
import { PrefixResultView } from "./PrefixResultView.tsx";
import { RangeResultView } from "./RangeResultView.tsx";

interface LookupResultProps {
  loading: boolean;
  error: string | null;
  payload: ApiPayload | null;
  isMyIp: boolean;
  onNavigate: (query: LookupQuery) => void;
  onCopied: (msg: string) => void;
}

function PayloadSwitch(props: {
  item: ApiPayload;
  isMyIp: boolean;
  onNavigate: (query: LookupQuery) => void;
  onCopied: (msg: string) => void;
}): JSX.Element {
  return (
    <Switch>
      <Match when={props.item.kind === "ip"}>
        <IpResultView
          data={(props.item as Extract<ApiPayload, { kind: "ip" }>).data}
          isMyIp={props.isMyIp}
          onNavigate={props.onNavigate}
          onCopied={props.onCopied}
        />
      </Match>
      <Match when={props.item.kind === "cidr"}>
        <PrefixResultView
          data={(props.item as Extract<ApiPayload, { kind: "cidr" }>).data}
          onNavigate={props.onNavigate}
          onCopied={props.onCopied}
        />
      </Match>
      <Match when={props.item.kind === "range"}>
        <RangeResultView
          data={(props.item as Extract<ApiPayload, { kind: "range" }>).data}
          onNavigate={props.onNavigate}
          onCopied={props.onCopied}
        />
      </Match>
      <Match when={props.item.kind === "asn"}>
        <AsnResultView
          data={(props.item as Extract<ApiPayload, { kind: "asn" }>).data}
          onNavigate={props.onNavigate}
          onCopied={props.onCopied}
        />
      </Match>
    </Switch>
  );
}

export function LookupResult(props: LookupResultProps): JSX.Element {
  return (
    <section class="result" aria-live="polite" aria-busy={props.loading}>
      <Show
        when={!props.loading}
        fallback={
          <div class="loading-panel">
            <div class="spinner" aria-hidden="true" />
            <span>Querying BGP &amp; RIR records…</span>
          </div>
        }
      >
        <Show
          when={!props.error}
          fallback={
            <div class="error-panel" role="alert">
              <Icon id="icon-alert" />
              <strong>Lookup failed</strong>
              <p>{props.error}</p>
            </div>
          }
        >
          <Show when={props.payload}>
            {(item) => (
              <PayloadSwitch
                item={item()}
                isMyIp={props.isMyIp}
                onNavigate={props.onNavigate}
                onCopied={props.onCopied}
              />
            )}
          </Show>
        </Show>
      </Show>
    </section>
  );
}
