import { Show, type JSX } from "solid-js";
import { formatRange } from "../../lib/ip.ts";
import type { LookupQuery, PrefixResponse } from "../../types/api.ts";
import { countryLabel } from "../ui/CountryFlag.tsx";
import { IdentityBanner, type IdentityChip } from "../ui/IdentityBanner.tsx";
import { ObjectListPanel } from "../ui/ObjectListPanel.tsx";
import { QueryLink } from "../ui/QueryLink.tsx";
import { RecordPanel } from "../ui/RecordPanel.tsx";
import { PaginationNav } from "./PaginationNav.tsx";

interface PrefixResultViewProps {
  data: PrefixResponse;
  onNavigate: (query: LookupQuery) => void;
  onCopied: (msg: string) => void;
}

function buildPrefixChips(data: PrefixResponse): IdentityChip[] {
  const chips: IdentityChip[] = [
    { text: data.prefix.version === 6 ? "IPv6" : "IPv4", accent: true },
    { text: `/${data.prefix.prefix_length}` },
    { text: `${data.prefix.address_count} addresses` }
  ];
  if (data.allocation?.registry) chips.push({ text: data.allocation.registry });
  if (data.allocation?.country_code) {
    chips.push({
      text: countryLabel(data.allocation.country_code) ?? data.allocation.country_code,
      countryCode: data.allocation.country_code
    });
  }
  return chips;
}

function PrefixPanels(props: { data: PrefixResponse; onNavigate: (q: LookupQuery) => void }): JSX.Element {
  return (
    <div class="record-grid">
      <RecordPanel
        title="Prefix details"
        icon="icon-network"
        fields={[
          { label: "Start IP", value: props.data.prefix.start_ip },
          { label: "End IP", value: props.data.prefix.end_ip },
          { label: "Addresses", value: props.data.prefix.address_count },
          { label: "Allocation", value: props.data.allocation?.name },
          { label: "Status", value: props.data.allocation?.status },
          { label: "Abuse", value: props.data.allocation?.abuse_contact }
        ]}
      />
      <ObjectListPanel
        title="Routing objects"
        icon="icon-route"
        count={props.data.routes.items.length}
        items={props.data.routes.items.map((r) => ({
          title: (
            <>
              <QueryLink query={{ kind: "cidr", value: r.prefix }} label={r.prefix} onNavigate={props.onNavigate} />
              <span class="query-separator"> · </span>
              <QueryLink query={{ kind: "asn", value: r.origin_asn }} label={r.origin_asn} onNavigate={props.onNavigate} />
            </>
          ),
          subtitle: [r.relation, r.organization ?? r.description, r.source].filter(Boolean).join(" · ") || null
        }))}
        emptyText="No covering or related BGP route objects found."
      />
    </div>
  );
}

export function PrefixResultView(props: PrefixResultViewProps): JSX.Element {
  return (
    <>
      <IdentityBanner
        kickerIcon="icon-route"
        kickerLabel="CIDR prefix"
        title={props.data.prefix.cidr}
        copyLabel={`Copy ${props.data.prefix.cidr}`}
        onCopied={props.onCopied}
        chips={buildPrefixChips(props.data)}
        rightLabel="Covering allocation"
        rightHeading={props.data.allocation?.name ?? "Unassigned / Multiple"}
        rightSubtitle={
          <QueryLink
            query={{ kind: "range", start: props.data.prefix.start_ip, end: props.data.prefix.end_ip, rangeKind: "allocations" }}
            label={formatRange(props.data.prefix.start_ip, props.data.prefix.end_ip)}
            onNavigate={props.onNavigate}
          />
        }
      />
      <PrefixPanels data={props.data} onNavigate={props.onNavigate} />
      <Show when={props.data.routes.next_cursor}>
        <PaginationNav
          routes={props.data.routes}
          onSelectPage={() => undefined}
          onNextCursor={(cursor) => props.onNavigate({ kind: "cidr", value: props.data.prefix.cidr, cursor })}
        />
      </Show>
    </>
  );
}
