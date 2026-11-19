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
    feature: "Base URL",
    full: "https://bgp-api.mehrnet.com",
    lite: "https://bgp-lite.mehrnet.com"
  },
  {
    feature: "IP lookup (/v1/ip/{ip})",
    full: "Full BGP, RIR, geolocation & lat/lng",
    lite: "Full BGP, RIR, country & fallback"
  },
  {
    feature: "Prefix & ASN paging",
    full: "Numbered & cursor pagination",
    lite: "Cursor pagination"
  },
  {
    feature: "Range query (/v1/range)",
    full: "Records + /0–/16 range summaries",
    lite: "Records (capped range window)"
  }
];

export const ENDPOINT_SPECS: EndpointSpec[] = [
  {
    id: "api-ip",
    title: "IP address lookup",
    summary: "Resolve BGP origin ASN, CIDR allocation, and geolocation for any IPv4 or IPv6 address.",
    method: "GET",
    pathTemplate: "/v1/ip/{ip}",
    exampleUrl: "https://bgp-api.mehrnet.com/v1/ip/1.1.1.1?details=true",
    parameters: [
      { name: "ip", location: "path", required: true, description: "IPv4 or IPv6 address to inspect." },
      { name: "details", location: "query", required: false, description: "Set true to include underlying source records." }
    ],
    responseJson: EXAMPLE_RESPONSES.ip
  },
  {
    id: "api-prefix",
    title: "CIDR prefix lookup",
    summary: "Inspect covering RIR allocation and announced origin routes for an IPv4 or IPv6 prefix.",
    method: "GET",
    pathTemplate: "/v1/prefix/{ip}/{prefix}",
    exampleUrl: "https://bgp-api.mehrnet.com/v1/prefix/1.1.1.0/24",
    parameters: [
      { name: "ip", location: "path", required: true, description: "Network base IP address." },
      { name: "prefix", location: "path", required: true, description: "Prefix length (0–32 IPv4, 0–128 IPv6)." },
      { name: "cursor", location: "query", required: false, description: "Pagination token from routes.next_cursor." }
    ],
    responseJson: EXAMPLE_RESPONSES.prefix
  },
  {
    id: "api-range",
    title: "IP range lookup",
    summary: "Query RIR allocations or BGP routes inside an inclusive IPv4 or IPv6 address range.",
    method: "GET",
    pathTemplate: "/v1/range",
    exampleUrl: "https://bgp-api.mehrnet.com/v1/range?start=1.1.1.0&end=1.1.1.255&kind=allocations",
    parameters: [
      { name: "start", location: "query", required: true, description: "Inclusive start IP address." },
      { name: "end", location: "query", required: true, description: "Inclusive end IP address (same IP family)." },
      { name: "kind", location: "query", required: false, description: "allocations (default) or routes." }
    ],
    responseJson: EXAMPLE_RESPONSES.range
  },
  {
    id: "api-asn",
    title: "ASN lookup",
    summary: "Retrieve autonomous system metadata and paginated announced prefixes.",
    method: "GET",
    pathTemplate: "/v1/asn/{asn}",
    exampleUrl: "https://bgp-api.mehrnet.com/v1/asn/AS13335?page=1&limit=50",
    parameters: [
      { name: "asn", location: "path", required: true, description: "AS number with or without AS prefix." },
      { name: "page", location: "query", required: false, description: "1-based page number when counts are available." },
      { name: "limit", location: "query", required: false, description: "Maximum routes per page (default 50, max 250)." }
    ],
    responseJson: EXAMPLE_RESPONSES.asn
  }
];
