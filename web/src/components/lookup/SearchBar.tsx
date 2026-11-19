import { Show, type JSX } from "solid-js";
import { Icon } from "../ui/Icon.tsx";

interface SearchBarProps {
  value: string;
  onInput: (val: string) => void;
  onSubmit: () => void;
  onMyIp: () => void;
  error: string | null;
  ipv6Candidate: string | null;
  onSelectIpv6: (ip: string) => void;
}

function Ipv6Banner(props: { ip: string | null; onSelect: (ip: string) => void }): JSX.Element {
  return (
    <Show when={props.ip}>
      {(ip) => (
        <button type="button" class="ipv6-button" onClick={() => props.onSelect(ip())}>
          <span class="ipv6-label">
            <span class="protocol-mark">IPv6 detected</span>
            <span id="ipv6-address">{ip()}</span>
          </span>
          <span class="ipv6-action">
            <span>Inspect IPv6</span>
            <Icon id="icon-arrow-right" />
          </span>
        </button>
      )}
    </Show>
  );
}

export function SearchBar(props: SearchBarProps): JSX.Element {
  const onSubmit = (event: SubmitEvent) => {
    event.preventDefault();
    props.onSubmit();
  };

  return (
    <section class="lookup-tools" aria-label="IP, prefix, or ASN lookup">
      <form class="lookup-form" novalidate onSubmit={onSubmit}>
        <label class="search-field" for="ip-input">
          <Icon id="icon-search" />
          <input
            id="ip-input"
            type="text"
            value={props.value}
            onInput={(e) => props.onInput(e.currentTarget.value)}
            placeholder="IP, CIDR prefix, AS13335, or start - end range"
            autocomplete="off"
            autocapitalize="off"
            spellcheck={false}
          />
        </label>
        <button class="primary-button" type="submit">
          <Icon id="icon-search" />
          <span>Lookup</span>
        </button>
        <button class="secondary-button" type="button" onClick={props.onMyIp}>
          <Icon id="icon-target" />
          <span>My IP</span>
        </button>
      </form>
      <Show when={props.error}>
        {(err) => <p class="lookup-error" role="status">{err()}</p>}
      </Show>
      <Ipv6Banner ip={props.ipv6Candidate} onSelect={props.onSelectIpv6} />
    </section>
  );
}
