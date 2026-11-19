import type { ResponseMeta } from "./lookup.ts";

export interface PrefixDescriptor {
  cidr: string;
  version: number;
  prefix_length: number;
  start_ip: string;
  end_ip: string;
  address_count: string;
}

export interface RangeDescriptor {
  start_ip: string;
  end_ip: string;
  version: number;
  address_count: string;
}

export interface RouteObject {
  prefix: string;
  version: number;
  origin_asn: string;
  as_number?: number | null;
  relation?: string | null;
  registry?: string | null;
  source?: string | null;
  maintainers?: string | null;
  organization?: string | null;
  abuse_contact?: string | null;
  description?: string | null;
}

export interface AllocationObject {
  start_ip: string;
  end_ip: string;
  version: number;
  registry?: string | null;
  country_code?: string | null;
  country_raw?: string | null;
  name?: string | null;
  status?: string | null;
  allocation_date?: string | null;
  created?: string | null;
  last_modified?: string | null;
  source?: string | null;
  maintainers?: string | null;
  organization?: string | null;
  abuse_contact?: string | null;
  description?: string | null;
}

export interface AutnumObject {
  asn: string;
  as_number: number;
  registry?: string | null;
  country_code?: string | null;
  country_raw?: string | null;
  name?: string | null;
  organization?: string | null;
  status?: string | null;
  created?: string | null;
  last_modified?: string | null;
  source?: string | null;
  maintainers?: string | null;
  abuse_contact?: string | null;
  description?: string | null;
}

export interface RoutePage {
  items: RouteObject[];
  next_cursor?: string | null;
  page?: number | null;
  total_pages?: number | null;
  total_items?: number | null;
}

export interface PrefixResponse {
  meta?: ResponseMeta | null;
  prefix: PrefixDescriptor;
  allocation?: AllocationObject | null;
  routes: RoutePage;
}

export interface RangeFacet {
  value: string;
  record_count: number;
}

export interface RangeSummaryDto {
  aggregation: string;
  bucket_prefix_length: number;
  buckets: number;
  allocation_records: number;
  route_records: number;
  countries: RangeFacet[];
  asns: RangeFacet[];
}

export interface RangeResponse {
  meta?: ResponseMeta | null;
  range: RangeDescriptor;
  kind: "allocations" | "routes";
  mode: string;
  summary?: RangeSummaryDto | null;
  allocations?: AllocationObject[];
  routes?: RouteObject[];
  next_cursor?: string | null;
}

export interface AsnResponse {
  meta?: ResponseMeta | null;
  asn: string;
  as_number: number;
  autnum?: AutnumObject | null;
  routes: RoutePage;
}
