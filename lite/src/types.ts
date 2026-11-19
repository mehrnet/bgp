export interface Env {
  CORS_ALLOWED_ORIGINS_JSON?: string;
}

export interface ExecutionContextLike {
  waitUntil?(promise: Promise<unknown>): void;
}

export interface ParsedIPv4 {
  isV6: false;
  value: number;
  canonical: string;
}

export interface ParsedIPv6 {
  isV6: true;
  value: bigint;
  canonical: string;
}

export type ParsedIP = ParsedIPv4 | ParsedIPv6;

export interface ParsedPrefixV4 {
  isV6: false;
  version: 4;
  bits: number;
  cidr: string;
  start: number;
  end: number;
  startIP: string;
  endIP: string;
  addressCount: string;
}

export interface ParsedPrefixV6 {
  isV6: true;
  version: 6;
  bits: number;
  cidr: string;
  start: bigint;
  end: bigint;
  startIP: string;
  endIP: string;
  addressCount: string;
}

export type ParsedPrefix = ParsedPrefixV4 | ParsedPrefixV6;

export interface DatasetMeta {
  release_tag?: string | null;
  built_at?: string | null;
  source_commit?: string | null;
}

export interface OffsetDict {
  isOffset: true;
  count: number;
  offsetsOff: number;
  dataOff: number;
}

export interface LazyDict {
  isOffset: false;
  offs: Uint32Array;
  lens: Uint32Array;
}

export type Dict = OffsetDict | LazyDict;

export interface SegTable {
  bits: number;
  kb: number;
  stride: number;
  shift: number;
  entries: number;
  blocks: number;
  indexOff: number;
  dataOff: number;
  dataLen: number;
}

export interface ChunkBuffer {
  bytes: Uint8Array;
  view: DataView;
}

export interface ChunkedSection {
  chunkSize: number;
  numChunks: number;
  offsetsOff: number;
  dataOff: number;
  cache: Map<number, ChunkBuffer>;
}

export interface AutnumRecord {
  asNumber: number;
  hasAutnum: boolean;
  registry?: string | null;
  country?: string | null;
  status?: string | null;
  source?: string | null;
  name?: string | null;
  organization?: string | null;
  maintainers?: string | null;
  abuseContact?: string | null;
  description?: string | null;
}

export interface OriginSummary {
  asn: string;
  as_number: number;
  name: string | null;
  organization: string | null;
}

export interface IpLookupResult {
  ip: string;
  version: 4 | 6;
  registry: string | null;
  allocation_date: null;
  allocation_status: null;
  network: {
    cidr: string | null;
    start_ip: string | null;
    end_ip: string | null;
    asn: string | null;
    asns: string[];
    origins: OriginSummary[];
    as_number: number | null;
    status: null;
    abuse_contact: null;
  };
  allocation: {
    start_ip: string | null;
    end_ip: string | null;
    registry: string | null;
    country_code: string | null;
    country_raw: string | null;
    name: null;
    allocation_date: null;
    status: null;
    abuse_contact: null;
  };
  location: {
    country_code: string | null;
    region: string | null;
    city: string | null;
    latitude: number | null;
    longitude: number | null;
  };
  sources: {
    allocation: boolean;
    route: boolean;
    geofeed: boolean;
  };
}

export interface RouteItem {
  prefix: string;
  version: 4 | 6;
  origin_asn: string;
  as_number: number;
  relation?: "exact" | "covering" | "more_specific";
  registry: string | null;
  source: string | null;
  maintainers: null;
  organization: string | null;
  abuse_contact: null;
  description: null;
}

export interface AllocationItem {
  start_ip: string;
  end_ip: string;
  version: 4 | 6;
  registry: string | null;
  country_code: string | null;
  country_raw: string | null;
  name: null;
  status: null;
  allocation_date: null;
  created: null;
  last_modified: null;
  source: null;
  maintainers: null;
  organization: null;
  abuse_contact: null;
  description: null;
}

export interface AutnumDto {
  asn: string;
  as_number: number;
  registry: string | null;
  country_code: string | null;
  country_raw: string | null;
  name: string | null;
  organization: string | null;
  status: string | null;
  created: null;
  last_modified: null;
  source: string | null;
  maintainers: string | null;
  abuse_contact: string | null;
  description: string | null;
}

export interface AsnLookupResult {
  asn: string;
  as_number: number;
  autnum: AutnumDto | null;
  items: RouteItem[];
  totalItems: number;
}

export type PrefixLookupResult =
  | { tooBroad: true }
  | { tooBroad?: false; allocation: AllocationItem | null; routes: RouteItem[]; nextCursor: string | null };
