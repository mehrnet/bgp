import { Show, type JSX } from "solid-js";
import { formatRange } from "../../lib/ip.ts";
import type { LookupQuery, RangeResponse, RangeSummaryDto } from "../../types/api.ts";
import { Icon } from "../ui/Icon.tsx";
import { IdentityBanner } from "../ui/IdentityBanner.tsx";
import { ObjectListPanel, type ObjectListItem } from "../ui/ObjectListPanel.tsx";
import { QueryLink } from "../ui/QueryLink.tsx";
import { RecordPanel } from "../ui/RecordPanel.tsx";

interface RangeResultViewProps {
  data: RangeResponse;
  onNavigate: (query: LookupQuery) => void;
  onCopied: (msg: string) => void;
}

function buildRangeItems(data: RangeResponse, onNavigate: (q: LookupQuery) => void): ObjectListItem[] {
  if (data.kind === "allocations") {
    return (data.allocations ?? []).map((a) => ({
      title: (
        <QueryLink
          query={{ kind: "range", start: a.start_ip, end: a.end_ip, rangeKind: "allocations" }}
          label={formatRange(a.start_ip, a.end_ip)}
          onNavigate={onNavigate}
        />
      ),
      subtitle: [a.registry, a.country_code, a.name ?? a.organization, a.status].filter(Boolean).join(" · ") || null
    }));
  }
  return (data.routes ?? []).map((r) => ({
    title: (
      <>
        <QueryLink query={{ kind: "cidr", value: r.prefix }} label={r.prefix} onNavigate={onNavigate} />
        <span class="query-separator"> · </span>
        <QueryLink query={{ kind: "asn", value: r.origin_asn }} label={r.origin_asn} onNavigate={onNavigate} />
      </>
    ),
    subtitle: [r.registry, r.organization ?? r.description, r.source].filter(Boolean).join(" · ") || null
  }));
}

function RangeSummaryGrid(props: { summary: RangeSummaryDto; onNavigate: (q: LookupQuery) => void }): JSX.Element {
  return (
    <div class="record-grid">
      <RecordPanel
        title="Aggregated summary"
        icon="icon-network"
        support={`/${props.summary.bucket_prefix_length} buckets`}
        fields={[
          { label: "Aggregation", value: props.summary.aggregation },
          { label: "Buckets", value: props.summary.buckets },
          { label: "Allocation records", value: props.summary.allocation_records },
          { label: "Route records", value: props.summary.route_records }
        ]}
      />
      <ObjectListPanel
        title="Top countries"
        icon="icon-globe"
        count={props.summary.countries.length}
        items={props.summary.countries.map((c) => ({ title: c.value, subtitle: `${c.record_count} records` }))}
      />
      <ObjectListPanel
        title="Top origin ASNs"
        icon="icon-network"
        count={props.summary.asns.length}
        items={props.summary.asns.map((a) => ({
          title: <QueryLink query={{ kind: "asn", value: a.value }} label={a.value} onNavigate={props.onNavigate} />,
          subtitle: `${a.record_count} routes`
        }))}
      />
    </div>
  );
}

function RangeModeToggle(props: { data: RangeResponse; onNavigate: (q: LookupQuery) => void }): JSX.Element {
  return (
    <div class="location-map-controls range-mode-bar">
      {(["allocations", "routes"] as const).map((kind) => (
        <button
          type="button"
          class="map-mode-button"
          aria-pressed={props.data.kind === kind}
          onClick={() => props.onNavigate({ kind: "range", start: props.data.range.start_ip, end: props.data.range.end_ip, rangeKind: kind })}
        >
          {kind === "allocations" ? "Allocations" : "Routes"}
        </button>
      ))}
    </div>
  );
}

export function RangeResultView(props: RangeResultViewProps): JSX.Element {
  const title = () => formatRange(props.data.range.start_ip, props.data.range.end_ip);
  const items = () => buildRangeItems(props.data, props.onNavigate);

  return (
    <>
      <IdentityBanner
        kickerIcon="icon-database"
        kickerLabel="Address range"
        title={title()}
        copyLabel={`Copy ${title()}`}
        onCopied={props.onCopied}
        chips={[
          { text: props.data.range.version === 6 ? "IPv6" : "IPv4", accent: true },
          { text: `${props.data.range.address_count} addresses` },
          { text: props.data.mode === "summary" ? "Summary" : props.data.kind }
        ]}
        rightLabel="View dataset"
        rightHeading={props.data.kind === "allocations" ? "RIR Allocations" : "BGP Routes"}
        rightExtra={<RangeModeToggle data={props.data} onNavigate={props.onNavigate} />}
      />
      <Show when={props.data.summary}>{(s) => <RangeSummaryGrid summary={s()} onNavigate={props.onNavigate} />}</Show>
      <Show when={props.data.mode !== "summary"}>
        <div class="record-grid record-grid-single">
          <ObjectListPanel title={props.data.kind === "allocations" ? "Allocations" : "Routes"} icon="icon-database" count={items().length} items={items()} />
        </div>
        <Show when={props.data.next_cursor}>
          {(cursor) => (
            <button type="button" class="more-button" onClick={() => props.onNavigate({ kind: "range", start: props.data.range.start_ip, end: props.data.range.end_ip, rangeKind: props.data.kind, cursor: cursor() })}>
              <span>Load next page</span>
              <Icon id="icon-arrow-right" />
            </button>
          )}
        </Show>
      </Show>
    </>
  );
}
