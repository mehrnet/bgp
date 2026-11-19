import type { LookupResponse } from "./lookup.ts";
import type { AsnResponse, PrefixResponse, RangeResponse } from "./resources.ts";

export * from "./lookup.ts";
export * from "./resources.ts";

export type LookupKind = "ip" | "cidr" | "asn" | "range";

export interface LookupQuery {
  kind: LookupKind;
  value?: string;
  start?: string;
  end?: string;
  rangeKind?: "allocations" | "routes";
  cursor?: string;
  page?: number;
  limit?: number;
}

export type ApiPayload =
  | { kind: "ip"; data: LookupResponse }
  | { kind: "cidr"; data: PrefixResponse }
  | { kind: "asn"; data: AsnResponse }
  | { kind: "range"; data: RangeResponse };
