import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { buildEndpointPath } from "../src/lib/apiClient.ts";
import { formatCodeSnippet } from "../src/lib/codeGen.ts";
import {
  buildPaginationTokens,
  classifyQuery,
  formatRange,
  isIPv4,
  isIPv6,
  networkAsns,
  parseAsn,
  parseCidr
} from "../src/lib/ip.ts";
import { parseRoute, queryToPath } from "../src/lib/routes.ts";

describe("ip validation and classification", () => {
  it("validates IPv4 and IPv6 addresses", () => {
    assert.equal(isIPv4("1.1.1.1"), true);
    assert.equal(isIPv4("256.1.1.1"), false);
    assert.equal(isIPv6("2001:4860:4860::8888"), true);
    assert.equal(isIPv6("::ffff:192.0.2.1"), true);
    assert.equal(isIPv6("2001:::1"), false);
  });

  it("classifies search queries", () => {
    assert.deepEqual(classifyQuery("1.1.1.1"), { kind: "ip", value: "1.1.1.1" });
    assert.deepEqual(classifyQuery("1.1.1.0/24"), { kind: "cidr", value: "1.1.1.0/24" });
    assert.deepEqual(classifyQuery("as13335"), { kind: "asn", value: "AS13335" });
    assert.deepEqual(classifyQuery("1.1.1.0 - 1.1.1.255"), {
      kind: "range",
      start: "1.1.1.0",
      end: "1.1.1.255",
      rangeKind: "allocations"
    });
    assert.equal(classifyQuery("not-an-ip"), null);
  });
});

describe("cidr, asn, and pagination helpers", () => {
  it("parses CIDR and ASN edge bounds", () => {
    assert.equal(parseCidr("8.8.8.0/33"), null);
    assert.equal(parseCidr("2001:db8::/32"), "2001:db8::/32");
    assert.equal(parseAsn("4294967296"), null);
    assert.equal(parseAsn("13335"), "AS13335");
  });

  it("deduplicates ASN lists and formats ranges and pages", () => {
    assert.deepEqual(
      networkAsns({
        asn: "AS13335",
        asns: ["as13335", "AS15169"],
        origins: [{ asn: "AS13335", as_number: 13335 }]
      }),
      ["AS13335", "AS15169"]
    );
    assert.equal(formatRange("1.0.0.0", "1.0.0.255"), "1.0.0.0 - 1.0.0.255");
    assert.deepEqual(buildPaginationTokens(1, 1), [1]);
    assert.deepEqual(buildPaginationTokens(5, 10), [1, "ellipsis", 3, 4, 5, 6, 7, "ellipsis", 10]);
  });
});

describe("routing and endpoint paths", () => {
  it("parses and serializes SPA routes", () => {
    const ipState = parseRoute("/ip/1.1.1.1", "");
    assert.equal(ipState.query?.kind, "ip");
    assert.equal(queryToPath(ipState.query!), "/ip/1.1.1.1");

    const cidrState = parseRoute("/cidr/2001:4860::/32", "");
    assert.deepEqual(cidrState.query, { kind: "cidr", value: "2001:4860::/32" });

    const apiState = parseRoute("/api/asn", "");
    assert.equal(apiState.view, "api");
    assert.equal(apiState.apiSection, "api-asn");
  });

  it("formats API endpoint paths and code snippets", () => {
    assert.equal(
      buildEndpointPath({ kind: "ip", value: "1.1.1.1" }),
      "/v1/ip/1.1.1.1?details=true"
    );
    assert.equal(
      buildEndpointPath({ kind: "asn", value: "AS13335", page: 2, limit: 25 }),
      "/v1/asn/AS13335?limit=25&page=2"
    );
    assert.match(formatCodeSnippet("curl", "https://bgp-api.mehrnet.com/v1/ip/1.1.1.1"), /curl -fsSL/);
  });
});
