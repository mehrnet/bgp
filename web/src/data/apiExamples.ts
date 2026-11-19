export const EXAMPLE_RESPONSES = {
  ip: `{
  "ip": "1.1.1.1",
  "version": 4,
  "registry": "APNIC",
  "network": {
    "cidr": "1.1.1.0/24",
    "asn": "AS13335"
  },
  "location": {
    "country_code": "AU",
    "latitude": -33.8688,
    "longitude": 151.2093
  }
}`,
  prefix: `{
  "prefix": {
    "cidr": "1.1.1.0/24",
    "version": 4,
    "prefix_length": 24,
    "address_count": "256"
  },
  "routes": {
    "items": [
      {
        "prefix": "1.1.1.0/24",
        "origin_asn": "AS13335"
      }
    ],
    "next_cursor": null
  }
}`,
  range: `{
  "range": {
    "start_ip": "1.1.1.0",
    "end_ip": "1.1.1.255",
    "version": 4,
    "address_count": "256"
  },
  "kind": "allocations",
  "mode": "records",
  "allocations": [
    {
      "start_ip": "1.1.1.0",
      "end_ip": "1.1.1.255",
      "registry": "APNIC"
    }
  ]
}`,
  asn: `{
  "asn": "AS13335",
  "as_number": 13335,
  "autnum": {
    "name": "CLOUDFLARENET",
    "registry": "ARIN",
    "country_code": "US"
  },
  "routes": {
    "items": [],
    "page": 1,
    "total_pages": 12
  }
}`
} as const;
