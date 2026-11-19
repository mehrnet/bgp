import { For, Show, type JSX } from "solid-js";
import { apiMode } from "../../lib/apiClient.ts";
import { formatCoord, formatRange, networkAsns, parseCoord } from "../../lib/ip.ts";
import type { LookupQuery, LookupResponse } from "../../types/api.ts";
import { countryLabel } from "../ui/CountryFlag.tsx";
import { IdentityBanner, type IdentityChip } from "../ui/IdentityBanner.tsx";
import { QueryLink } from "../ui/QueryLink.tsx";
import { RecordPanel } from "../ui/RecordPanel.tsx";
import { LocationMap } from "./LocationMap.tsx";

interface IpResultViewProps {
  data: LookupResponse;
  isMyIp: boolean;
  onNavigate: (query: LookupQuery) => void;
  onCopied: (msg: string) => void;
}

function buildIpChips(data: LookupResponse): IdentityChip[] {
  const cc = data.location?.country_code ?? data.allocation?.country_code;
  const registry = data.registry ?? data.allocation?.registry;
  const status = data.allocation_status ?? data.allocation?.status;
  const chips: IdentityChip[] = [{ text: data.version === 6 ? "IPv6" : "IPv4", accent: true }];
  if (registry) chips.push({ text: registry });
  if (cc) chips.push({ text: countryLabel(cc) ?? cc, countryCode: cc });
  if (status) chips.push({ text: status });
  return chips;
}

function OriginsSubtitle(props: { data: LookupResponse; onNavigate: (q: LookupQuery) => void }): JSX.Element {
  const asns = () => networkAsns(props.data.network);
  return (
    <>
      <Show when={props.data.network?.cidr}>
        {(cidr) => (
          <>
            <span class="network-subtitle-label">Route</span>
            <QueryLink query={{ kind: "cidr", value: cidr() }} label={cidr()} onNavigate={props.onNavigate} />
          </>
        )}
      </Show>
      <Show when={asns().length > 0}>
        <span class="network-subtitle-label">{asns().length > 1 ? "Origins" : "Origin"}</span>
        <For each={asns()}>
          {(asn, idx) => {
            const org = props.data.network?.origins?.find((o) => o.asn.toUpperCase() === asn);
            const orgName = org?.organization ?? org?.name;
            return (
              <>
                <Show when={idx() > 0}><span class="query-separator">/</span></Show>
                <span class="route-origin">
                  <QueryLink query={{ kind: "asn", value: asn }} label={asn} onNavigate={props.onNavigate} />
                  <Show when={orgName}>{(n) => <span class="route-origin-name">{n()}</span>}</Show>
                </span>
              </>
            );
          }}
        </For>
      </Show>
    </>
  );
}

function routeFields(data: LookupResponse, onNavigate: (q: LookupQuery) => void) {
  const asns = networkAsns(data.network);
  return [
    {
      label: "Prefix",
      value: data.network?.cidr
        ? <QueryLink query={{ kind: "cidr", value: data.network.cidr }} label={data.network.cidr} onNavigate={onNavigate} />
        : "Unannounced"
    },
    {
      label: "Origin ASNs",
      value: asns.length > 0
        ? <For each={asns}>{(a, i) => <>{i() > 0 && <span class="query-separator">, </span>}<QueryLink query={{ kind: "asn", value: a }} label={a} onNavigate={onNavigate} /></>}</For>
        : "Unavailable"
    },
    { label: "Range", value: formatRange(data.network?.start_ip, data.network?.end_ip) },
    { label: "Status", value: data.network?.status },
    { label: "Abuse contact", value: data.network?.abuse_contact }
  ];
}

function allocationFields(data: LookupResponse, onNavigate: (q: LookupQuery) => void) {
  const alloc = data.allocation;
  const rangeLabel = formatRange(alloc?.start_ip, alloc?.end_ip);
  const canQueryRange = alloc?.start_ip && alloc?.end_ip && apiMode() === "full";
  return [
    { label: "Network name", value: alloc?.name },
    {
      label: "Range",
      value: canQueryRange
        ? <QueryLink query={{ kind: "range", start: alloc.start_ip!, end: alloc.end_ip!, rangeKind: "allocations" }} label={rangeLabel} onNavigate={onNavigate} />
        : rangeLabel
    },
    { label: "Date", value: data.allocation_date ?? alloc?.allocation_date },
    { label: "Status", value: data.allocation_status ?? alloc?.status },
    { label: "Abuse contact", value: alloc?.abuse_contact }
  ];
}

function IpRecordPanels(props: { data: LookupResponse; onNavigate: (q: LookupQuery) => void }): JSX.Element {
  const lat = () => parseCoord(props.data.location?.latitude);
  const lng = () => parseCoord(props.data.location?.longitude);

  return (
    <div class="record-grid">
      <RecordPanel title="BGP route" icon="icon-route" support={props.data.sources?.route ? "Active" : "No route"} fields={routeFields(props.data, props.onNavigate)} />
      <RecordPanel title="Allocation" icon="icon-registry" support={props.data.registry ?? props.data.allocation?.registry ?? null} fields={allocationFields(props.data, props.onNavigate)} />
      <RecordPanel
        title="Geolocation"
        icon="icon-globe"
        support={props.data.sources?.geofeed ? "Geofeed" : null}
        fields={[
          { label: "Country", value: countryLabel(props.data.location?.country_code ?? props.data.allocation?.country_code) },
          { label: "Region", value: props.data.location?.region },
          { label: "City", value: props.data.location?.city },
          { label: "Latitude", value: lat() !== null ? formatCoord(lat()!) : null },
          { label: "Longitude", value: lng() !== null ? formatCoord(lng()!) : null }
        ]}
      />
    </div>
  );
}

export function IpResultView(props: IpResultViewProps): JSX.Element {
  const lat = () => parseCoord(props.data.location?.latitude);
  const lng = () => parseCoord(props.data.location?.longitude);
  const place = () =>
    [props.data.location?.city, props.data.location?.region, countryLabel(props.data.location?.country_code ?? props.data.allocation?.country_code)]
      .filter(Boolean).join(", ") || props.data.ip;

  return (
    <>
      <IdentityBanner
        kickerIcon="icon-network"
        kickerLabel={props.isMyIp ? "Your IP address" : "IP address"}
        title={props.data.ip}
        copyLabel={`Copy ${props.data.ip}`}
        onCopied={props.onCopied}
        chips={buildIpChips(props.data)}
        rightLabel="Network & Organization"
        rightHeading={props.data.allocation?.name ?? props.data.network?.origins?.[0]?.organization ?? props.data.network?.origins?.[0]?.name ?? props.data.network?.cidr ?? "Unannounced"}
        rightSubtitle={<OriginsSubtitle data={props.data} onNavigate={props.onNavigate} />}
      />
      <IpRecordPanels data={props.data} onNavigate={props.onNavigate} />
      <Show when={lat() !== null && lng() !== null}>
        <LocationMap latitude={lat()!} longitude={lng()!} placeLabel={place()} />
      </Show>
    </>
  );
}
