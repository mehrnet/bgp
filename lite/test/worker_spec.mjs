import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { LiteDB } from "../src/mnl1.js";
import { handleRequest } from "../src/handler.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../..");

function padV4(ip) {
  const parts = ip.split(".").map(Number);
  const v = ((0xffffn << 32n) |
    (BigInt(parts[0]) << 24n) |
    (BigInt(parts[1]) << 16n) |
    (BigInt(parts[2]) << 8n) |
    BigInt(parts[3])).toString();
  return v.padStart(39, "0");
}

function padV6(valBig) {
  return valBig.toString().padStart(39, "0");
}

function buildFixtureDB() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "bgp-lite-test-"));
  try {
    const extraAutnums = [];
    for (let asn = 65000; asn <= 65300; asn++) {
      extraAutnums.push(`AS${asn},ripe,DE,NET-${asn},ORG-${asn},ASSIGNED,,,,RIPE,,`);
    }
    fs.writeFileSync(
      path.join(tmp, "autnums.csv"),
      [
        "AS13335,arin,US,CLOUDFLARENET,ORG-CLOUD14-ARIN,ASSIGNED,,,ARIN,ARIN-HM,abuse@cloudflare.com,Cloudflare Inc.",
        "AS64512,ripe,DE,PRIV-NET,ORG-PRIV,ASSIGNED,,,,RIPE,,",
        ...extraAutnums,
      ].join("\n") + "\n"
    );
    const v6Start = (0x2606n << 112n) | (0x4700n << 96n);
    const v6End = v6Start | ((1n << 96n) - 1n);
    const v6SubStart = v6Start | (0x4700n << 80n);
    const v6SubEnd = v6SubStart | ((1n << 80n) - 1n);

    const v6Sub64Start = v6SubStart | (1n << 64n);
    const v6Sub64End = v6Sub64Start | ((1n << 64n) - 1n);

    fs.writeFileSync(
      path.join(tmp, "allocations.csv"),
      [
        `${padV4("1.0.0.0")},${padV4("1.255.255.255")},4,apnic,AU,apnic-net,,,,,,,,,`,
        `${padV4("1.1.2.0")},${padV4("1.1.2.255")},4,apnic,SG,sg-net,,,,,,,,,`,
        `${padV4("1.1.2.10")},${padV4("1.1.2.25")},4,apnic,JP,jp-sub,,,,,,,,,`,
        `${padV6(v6Start)},${padV6(v6End)},6,arin,US,cf-v6,,,,,,,,,`,
        `${padV6(v6Sub64Start)},${padV6(v6Sub64End)},6,arin,CA,cf-v6-64,,,,,,,,,`,
      ].join("\n") + "\n"
    );
    fs.writeFileSync(
      path.join(tmp, "geolocations.csv"),
      [
        `${padV4("1.1.1.0")},${padV4("1.1.1.255")},4,US,US-CA,Los Angeles`,
        `${padV6(v6SubStart)},${padV6(v6SubEnd)},6,GB,GB-ENG,London`,
      ].join("\n") + "\n"
    );
    fs.writeFileSync(
      path.join(tmp, "routes.csv"),
      [
        `${padV4("1.1.0.0")},${padV4("1.1.255.255")},4,1.1.0.0/16,AS64512,ripe,,,,,`,
        `${padV4("1.1.1.0")},${padV4("1.1.1.255")},4,1.1.1.0/24,AS13335,apnic,,,,,`,
        `${padV4("1.1.2.0")},${padV4("1.1.2.255")},4,1.1.2.0/24,AS13335,apnic,,,,,`,
        `${padV4("1.1.2.0")},${padV4("1.1.2.255")},4,1.1.2.0/24,AS64512,ripe,,,,,`,
        `${padV4("1.1.2.128")},${padV4("1.1.2.255")},4,1.1.2.128/25,AS65300,ripe,,,,,`,
        `${padV4("2.0.0.0")},${padV4("2.0.0.255")},4,2.0.0.0/24,AS13335,ripe,,,,,`,
        `${padV4("2.0.1.0")},${padV4("2.0.1.255")},4,2.0.1.0/24,AS13335,ripe,,,,,`,
        `${padV6(v6Start)},${padV6(v6End)},6,2606:4700::/32,AS13335,arin,,,,,`,
        `${padV6(v6SubStart)},${padV6(v6SubEnd)},6,2606:4700:4700::/48,AS64512,ripe,,,,,`,
      ].join("\n") + "\n"
    );

    const out = path.join(tmp, "bgp-lite.mnl");
    execFileSync(
      "cargo",
      [
        "run",
        "--quiet",
        "-p",
        "bgp-tools",
        "--bin",
        "build-lite-db",
        "--",
        "-input",
        tmp,
        "-output",
        out,
        "-release-tag",
        "db-test-1",
        "-built-at",
        "2026-11-17T10:00:00Z",
        "-source-commit",
        "deadbeef",
      ],
      { cwd: repoRoot, env: { ...process.env, TMPDIR: "/var/tmp" } }
    );
    return new LiteDB(fs.readFileSync(out));
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

const db = buildFixtureDB();

async function call(pathAndQuery, init = {}) {
  const req = new Request(`https://bgp-lite.mehrnet.com${pathAndQuery}`, init);
  const res = await handleRequest(db, req, {});
  const text = await res.text();
  return { status: res.status, headers: res.headers, text, json: () => JSON.parse(text) };
}

test("health and root endpoints", async () => {
  const r = await call("/v1/health", { headers: { Origin: "https://bgp.mehrnet.com" } });
  assert.equal(r.status, 200);
  assert.equal(r.headers.get("Access-Control-Allow-Origin"), "https://bgp.mehrnet.com");
  const body = r.json();
  assert.equal(body.ok, true);
  assert.equal(body.service, "bgp-api");
  assert.equal(body.mode, "lite");
  assert.equal(body.database, "lite");
  assert.equal(body.dataset.release_tag, "db-test-1");
});

test("v1/ip IPv4, IPv6, multi-origin, sibling /24, and v4-mapped", async () => {
  const r1 = await call("/v1/ip?query=1.1.1.1");
  assert.equal(r1.status, 200);
  const b1 = r1.json();
  assert.equal(b1.ip, "1.1.1.1");
  assert.equal(b1.registry, "apnic");
  assert.equal(b1.network.cidr, "1.1.1.0/24");
  assert.equal(b1.network.asn, "AS13335");
  assert.equal(b1.network.as_number, 13335);
  assert.equal(b1.network.origins[0].name, "CLOUDFLARENET");
  assert.equal(b1.allocation.country_code, "AU");
  assert.equal(b1.location.country_code, "US");
  assert.equal(b1.location.region, "US-CA");
  assert.equal(b1.location.city, "Los Angeles");
  assert.deepEqual(b1.sources, { allocation: true, route: true, geofeed: true });

  // Multi-origin
  const r2 = await call("/v1/ip?query=1.1.2.9");
  const b2 = r2.json();
  assert.equal(b2.network.asn, null);
  assert.equal(b2.network.as_number, null);
  assert.deepEqual(b2.network.asns, ["AS13335", "AS64512"]);

  // Sub-/24 non-CIDR allocation
  const rSub = (await call("/v1/ip?query=1.1.2.15")).json();
  assert.equal(rSub.allocation.country_code, "JP");
  assert.equal(rSub.allocation.start_ip, "1.1.2.10");
  assert.equal(rSub.allocation.end_ip, "1.1.2.25");

  // IPv6 /64 sub-allocation
  const r6Sub = (await call("/v1/ip?query=2606:4700:4700:1::42")).json();
  assert.equal(r6Sub.allocation.country_code, "CA");

  // Sibling /24 must not collapse
  const r3 = await call("/v1/ip?query=2.0.1.42");
  assert.equal(r3.json().network.cidr, "2.0.1.0/24");

  // IPv6
  const r6 = await call("/v1/ip?query=2606:4700:4700::1");
  const b6 = r6.json();
  assert.equal(b6.version, 6);
  assert.equal(b6.network.cidr, "2606:4700:4700::/48");
  assert.equal(b6.network.asn, "AS64512");
  assert.equal(b6.location.country_code, "GB");
  assert.equal(b6.location.region, "GB-ENG");
  assert.equal(b6.location.city, "London");

  // IPv4-mapped IPv6
  const rm = await call("/v1/ip?query=::ffff:1.1.1.1");
  assert.equal(rm.json().ip, "1.1.1.1");
  assert.equal(rm.json().version, 4);

  // Plain-text shortcut
  const rc = await call("/v1/country?query=1.1.1.1");
  assert.equal(rc.status, 200);
  assert.equal(rc.text.trim(), "US");
});

test("v1/asn and v1/prefix", async () => {
  const a1 = await call("/v1/asn?query=AS13335&page=1&limit=2");
  assert.equal(a1.status, 200);
  const ab = a1.json();
  assert.equal(ab.autnum.name, "CLOUDFLARENET");
  assert.equal(ab.autnum.maintainers, "ARIN-HM");
  assert.equal(ab.autnum.abuse_contact, "abuse@cloudflare.com");
  assert.equal(ab.autnum.description, "Cloudflare Inc.");
  assert.equal(ab.routes.items.length, 2);
  assert.equal(ab.routes.total_items, 5);
  assert.equal(ab.routes.total_pages, 3);

  const p1 = await call("/v1/prefix?prefix=1.1.0.0/16");
  assert.equal(p1.status, 200);
  const pb = p1.json();
  assert.equal(pb.prefix.cidr, "1.1.0.0/16");
  assert.equal(pb.allocation.country_code, "AU");
  assert.ok(pb.routes.items.length >= 3);

  // Chunk > 0 ASN + unaligned /25 shifted-delta route
  const aChunk1 = await call("/v1/asn?query=AS65300");
  assert.equal(aChunk1.status, 200);
  assert.equal(aChunk1.json().autnum.name, "NET-65300");
  assert.equal(aChunk1.json().routes.items[0].prefix, "1.1.2.128/25");
  const rUnaligned = await call("/v1/ip?query=1.1.2.200");
  assert.equal(rUnaligned.json().network.cidr, "1.1.2.128/25");
  assert.equal(rUnaligned.json().network.origins[0].name, "NET-65300");
});

test("validation and 404 errors match contract", async () => {
  assert.equal((await call("/v1/ip?query=not-an-ip")).json().error.code, "INVALID_IP");
  assert.equal((await call("/v1/ip?query=9.9.9.9")).json().error.code, "IP_NOT_FOUND");
  assert.equal((await call("/v1/asn?query=abc")).json().error.code, "INVALID_ASN");
  assert.equal((await call("/v1/asn?query=AS999999")).json().error.code, "ASN_NOT_FOUND");
  assert.equal((await call("/v1/asn?query=13335&page=1&cursor=0")).json().error.code, "INVALID_PAGINATION");
  assert.equal((await call("/v1/prefix?prefix=bad")).json().error.code, "INVALID_PREFIX");
});
