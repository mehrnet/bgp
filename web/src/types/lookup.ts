export interface DatasetMetadata {
  release_tag?: string | null;
  built_at?: string | null;
  activated_at?: string | null;
  source_commit?: string | null;
}

export interface ResponseMeta {
  dataset?: DatasetMetadata | null;
}

export interface AsnIdentity {
  asn: string;
  as_number: number;
  name?: string | null;
  organization?: string | null;
}

export interface LookupNetwork {
  cidr?: string | null;
  start_ip?: string | null;
  end_ip?: string | null;
  asn?: string | null;
  asns?: string[];
  origins?: AsnIdentity[];
  as_number?: number | null;
  status?: string | null;
  abuse_contact?: string | null;
}

export interface LookupAllocation {
  start_ip?: string | null;
  end_ip?: string | null;
  registry?: string | null;
  country_code?: string | null;
  country_raw?: string | null;
  name?: string | null;
  allocation_date?: string | null;
  status?: string | null;
  abuse_contact?: string | null;
}

export interface LookupLocation {
  country_code?: string | null;
  region?: string | null;
  city?: string | null;
  latitude?: number | null;
  longitude?: number | null;
}

export interface LookupSources {
  allocation?: boolean;
  route?: boolean;
  geofeed?: boolean;
}

export interface LookupDetailRecord {
  start_ip: string;
  end_ip: string;
  version: number;
  registry?: string | null;
  country_code?: string | null;
  country_raw?: string | null;
  name?: string | null;
  cidr?: string | null;
  asn?: string | null;
  as_number?: number | null;
  region?: string | null;
  city?: string | null;
  latitude?: number | null;
  longitude?: number | null;
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

export interface LookupDetails {
  allocations?: LookupDetailRecord[];
  routes?: LookupDetailRecord[];
  geofeeds?: LookupDetailRecord[];
}

export interface LookupResponse {
  meta?: ResponseMeta | null;
  ip: string;
  version: number;
  registry?: string | null;
  allocation_date?: string | null;
  allocation_status?: string | null;
  network?: LookupNetwork;
  allocation?: LookupAllocation;
  location?: LookupLocation;
  sources?: LookupSources;
  details?: LookupDetails | null;
}
