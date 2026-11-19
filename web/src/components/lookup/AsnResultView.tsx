import { Show, type JSX } from "solid-js";
import type { AsnResponse, AutnumObject, LookupQuery } from "../../types/api.ts";
import { countryLabel } from "../ui/CountryFlag.tsx";
import { IdentityBanner, type IdentityChip } from "../ui/IdentityBanner.tsx";
import { ObjectListPanel } from "../ui/ObjectListPanel.tsx";
import { QueryLink } from "../ui/QueryLink.tsx";
import { PaginationNav } from "./PaginationNav.tsx";

interface AsnResultViewProps {
  data: AsnResponse;
  onNavigate: (query: LookupQuery) => void;
  onCopied: (msg: string) => void;
}

function buildAsnChips(data: AsnResponse): IdentityChip[] {
  const chips: IdentityChip[] = [{ text: `AS${data.as_number}`, accent: true }];
  const autnum = data.autnum;
  if (autnum?.registry) chips.push({ text: autnum.registry });
  if (autnum?.country_code) {
    chips.push({
      text: countryLabel(autnum.country_code) ?? autnum.country_code,
      countryCode: autnum.country_code
    });
  }
  if (autnum?.status) chips.push({ text: autnum.status });
  return chips;
}

function AsnDetailList(props: { autnum: AutnumObject | null | undefined }): JSX.Element {
  return (
    <Show when={props.autnum}>
      {(obj) => (
        <dl class="asn-detail-list">
          <Show when={obj().name}>
            <div class="record-row"><dt>AS name</dt><dd>{obj().name}</dd></div>
          </Show>
          <Show when={obj().abuse_contact}>
            <div class="record-row"><dt>Abuse</dt><dd>{obj().abuse_contact}</dd></div>
          </Show>
          <Show when={obj().source}>
            <div class="record-row"><dt>Source</dt><dd>{obj().source}</dd></div>
          </Show>
        </dl>
      )}
    </Show>
  );
}

export function AsnResultView(props: AsnResultViewProps): JSX.Element {
  return (
    <>
      <IdentityBanner
        kickerIcon="icon-network"
        kickerLabel="Autonomous system"
        title={props.data.asn}
        copyLabel={`Copy ${props.data.asn}`}
        onCopied={props.onCopied}
        chips={buildAsnChips(props.data)}
        rightClass="asn-identity-block"
        rightLabel="Organization"
        rightHeading={props.data.autnum?.organization ?? props.data.autnum?.name ?? props.data.asn}
        rightExtra={<AsnDetailList autnum={props.data.autnum} />}
      />
      <div class="record-grid record-grid-single">
        <ObjectListPanel
          title="Announced prefixes"
          icon="icon-route"
          count={props.data.routes.total_items ?? props.data.routes.items.length}
          items={props.data.routes.items.map((r) => ({
            title: <QueryLink query={{ kind: "cidr", value: r.prefix }} label={r.prefix} onNavigate={props.onNavigate} />,
            subtitle: [r.registry, r.source, r.description ?? r.organization].filter(Boolean).join(" · ") || null
          }))}
          emptyText="No announced prefixes found for this ASN."
        />
      </div>
      <PaginationNav
        routes={props.data.routes}
        onSelectPage={(page) => props.onNavigate({ kind: "asn", value: props.data.asn, page })}
        onNextCursor={(cursor) => props.onNavigate({ kind: "asn", value: props.data.asn, cursor })}
      />
    </>
  );
}
