import { EXAMPLE_RESPONSES } from "./apiExamples.ts";

export type DocLanguage = "curl" | "javascript" | "php" | "python" | "go";

export interface EndpointParam {
  name: string;
  location: "path" | "query";
  required: boolean;
  description: string;
}

export interface EndpointSpec {
  id: string;
  title: string;
  availability: string;
  summary: string;
  method: "GET";
  pathTemplate: string;
  exampleUrl: string;
  parameters: EndpointParam[];
  responseJson: string;
}

export interface TierRow {
  feature: string;
  full: string;
  lite: string;
}

export const LANGUAGE_OPTIONS: { id: DocLanguage; label: string; icon: string }[] = [
  { id: "curl", label: "cURL", icon: "icon-language-curl" },
  { id: "javascript", label: "JavaScript", icon: "icon-language-javascript" },
  { id: "php", label: "PHP", icon: "icon-language-php" },
  { id: "python", label: "Python", icon: "icon-language-python" },
  { id: "go", label: "Go", icon: "icon-language-go" }
];

export const TIER_COMPARISON: TierRow[] = [
  {
    feature: "IP lookup (/v1/ip?query=)",
    full: "Full route, allocation, geofeed, WHOIS netname/abuse/dates, and ?details=full",
    lite: "Route (cidr, asns, origins), /32 v4 & /64 v6 allocations, geofeed coords"
  },
  {
    feature: "Country shortcut (/v1/country)",
    full: "Not available",
    lite: "Plain-text ISO country code (or JSON with format=json)"
  },
  {
    feature: "ASN lookup (/v1/asn?query=)",
    full: "Full aut-num record + paginated route objects with IRR metadata",
    lite: "aut-num metadata (name, org, registry, country, abuse) + paginated prefixes"
  },
  {
    feature: "Prefix lookup (/v1/prefix?prefix=)",
    full: "Covering allocation + all overlapping IRR route objects",
    lite: "Covering allocation + winning route objects"
  },
  {
    feature: "Range lookup (/v1/range)",
    full: "Broad /0–/16 summaries & cursor-paginated overlap scans",
    lite: "Not available (returns 404)"
  }
];

export const DIRECTORY_LINKS: { href: string; method: string; path: string; note: string }[] = [
  { href: "#api-ip", method: "GET", path: "/v1/ip?query={address}", note: "IPv4 or IPv6 address (Full & Lite)" },
  { href: "#api-prefix", method: "GET", path: "/v1/prefix?prefix={cidr}", note: "Allocation & route objects (Full & Lite)" },
  { href: "#api-range", method: "GET", path: "/v1/range?start={ip}&end={ip}", note: "Overlapping blocks or routes (Full only)" },
  { href: "#api-asn", method: "GET", path: "/v1/asn?query={asn}", note: "Aut-num record & prefixes (Full & Lite)" },
  { href: "#api-country", method: "GET", path: "/v1/country?query={address}", note: "Plain-text or JSON country code (Lite only)" },
  { href: "#api-overview", method: "GET", path: "/v1/health", note: "Service health and dataset metadata (Full & Lite)" }
];

export const ENDPOINT_SPECS: EndpointSpec[] = [
  {
    id: "api-ip",
    title: "IP address lookup",
    availability: "Full & Lite API",
    summary: "Resolve an IPv4 or IPv6 address into its best matching route, allocation, ASN, and geolocation.",
    method: "GET",
    pathTemplate: "/v1/ip?query={address}",
    exampleUrl: "https://bgp-api.mehrnet.com/v1/ip?query=1.1.1.1",
    parameters: [
      { name: "query", location: "query", required: true, description: "IPv4 or IPv6 address." },
      { name: "details", location: "query", required: false, description: "Set full (Full API) to return raw matched source records." }
    ],
    responseJson: EXAMPLE_RESPONSES.ip
  },
  {
    id: "api-prefix",
    title: "CIDR prefix lookup",
    availability: "Full & Lite API",
    summary: "Inspect covering RIR allocation and registered route objects for an IPv4 or IPv6 CIDR.",
    method: "GET",
    pathTemplate: "/v1/prefix?prefix={cidr}",
    exampleUrl: "https://bgp-api.mehrnet.com/v1/prefix?prefix=1.1.1.0%2F24&limit=10",
    parameters: [
      { name: "prefix", location: "query", required: true, description: "IPv4 or IPv6 CIDR block." },
      { name: "limit", location: "query", required: false, description: "1–100 route items per page." },
      { name: "cursor", location: "query", required: false, description: "Continuation cursor from routes.next_cursor." }
    ],
    responseJson: EXAMPLE_RESPONSES.prefix
  },
  {
    id: "api-range",
    title: "Range lookup",
    availability: "Full API only",
    summary: "Find allocations or route objects overlapping an explicit inclusive start and end address.",
    method: "GET",
    pathTemplate: "/v1/range?start={ip}&end={ip}",
    exampleUrl: "https://bgp-api.mehrnet.com/v1/range?start=1.1.1.0&end=1.1.1.255&kind=routes&limit=10",
    parameters: [
      { name: "start", location: "query", required: true, description: "Range start IP address." },
      { name: "end", location: "query", required: true, description: "Range end IP address." },
      { name: "kind", location: "query", required: false, description: "allocations (default) or routes." },
      { name: "limit", location: "query", required: false, description: "1–100 items per page." }
    ],
    responseJson: EXAMPLE_RESPONSES.range
  },
  {
    id: "api-asn",
    title: "ASN lookup",
    availability: "Full & Lite API",
    summary: "Fetch an autonomous system aut-num record and paginated registered route objects.",
    method: "GET",
    pathTemplate: "/v1/asn?query={asn}",
    exampleUrl: "https://bgp-api.mehrnet.com/v1/asn?query=AS13335&limit=10&page=1",
    parameters: [
      { name: "query", location: "query", required: true, description: "AS number with or without AS prefix." },
      { name: "limit", location: "query", required: false, description: "1–100 routes per page." },
      { name: "page", location: "query", required: false, description: "1-based page number (Full & Lite). Do not combine with cursor." },
      { name: "cursor", location: "query", required: false, description: "Continuation cursor." }
    ],
    responseJson: EXAMPLE_RESPONSES.asn
  },
  {
    id: "api-country",
    title: "Country shortcut (Lite edge worker)",
    availability: "Lite API only",
    summary: "Low-latency ISO 3166-1 alpha-2 country lookup via bgp-lite.mehrnet.com (defaults to caller IP).",
    method: "GET",
    pathTemplate: "/v1/country?query={address}",
    exampleUrl: "https://bgp-lite.mehrnet.com/v1/country?query=1.1.1.1&format=json",
    parameters: [
      { name: "query", location: "query", required: false, description: "IPv4 or IPv6 address (defaults to connecting client IP)." },
      { name: "format", location: "query", required: false, description: "Use json for JSON payload instead of plain text country code." }
    ],
    responseJson: EXAMPLE_RESPONSES.country
  }
];
