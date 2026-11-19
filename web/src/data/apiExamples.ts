export const EXAMPLE_RESPONSES = {
  ip: `{
  "meta": { "dataset": { "release_tag": "db-2026.11.18-1654-168" } },
  "ip": "1.1.1.1",
  "version": 4,
  "registry": "apnic",
  "network": {
    "cidr": "1.1.1.0/24",
    "asn": "AS13335",
    "origins": [{ "asn": "AS13335", "name": "CLOUDFLARENET" }]
  },
  "allocation": { "country_code": "AU", "registry": "apnic" },
  "location": { "country_code": "AU", "latitude": -33.8688, "longitude": 151.2093 },
  "sources": { "allocation": true, "route": true, "geofeed": false }
}`,
  prefix: `{
  "prefix": { "cidr": "1.1.1.0/24", "version": 4, "address_count": "256" },
  "allocation": { "registry": "apnic", "country_code": "AU", "name": "APNIC-LABS" },
  "routes": {
    "items": [{ "prefix": "1.1.1.0/24", "origin_asn": "AS13335", "relation": "exact" }],
    "next_cursor": null
  }
}`,
  range: `{
  "range": { "start_ip": "1.1.1.0", "end_ip": "1.1.1.255", "version": 4, "address_count": "256" },
  "kind": "routes",
  "mode": "records",
  "routes": [{ "prefix": "1.1.1.0/24", "origin_asn": "AS13335", "registry": "apnic" }],
  "next_cursor": null
}`,
  asn: `{
  "asn": "AS13335",
  "as_number": 13335,
  "autnum": { "name": "CLOUDFLARENET", "registry": "arin", "source": "ARIN" },
  "routes": {
    "items": [{ "prefix": "1.1.1.0/24", "origin_asn": "AS13335", "version": 4 }],
    "next_cursor": null,
    "page": 1,
    "total_pages": 6
  }
}`,
  country: `{
  "ip": "1.1.1.1",
  "country_code": "AU"
}`
} as const;
