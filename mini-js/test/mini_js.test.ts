import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { buildMiniJsBundles } from "../src/build.ts";
import type { AsnMeta } from "../src/runtime/asn-meta.ts";
import type { AsnLookup } from "../src/runtime/asn.ts";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../..");

function padV4(ip: string): string {
  const parts = ip.split(".").map(Number);
  const v = (
    (0xffffn << 32n) |
    (BigInt(parts[0] ?? 0) << 24n) |
    (BigInt(parts[1] ?? 0) << 16n) |
    (BigInt(parts[2] ?? 0) << 8n) |
    BigInt(parts[3] ?? 0)
  ).toString();
  return v.padStart(39, "0");
}

function padV6(valBig: bigint): string {
  return valBig.toString().padStart(39, "0");
}

test("builds and executes country.min.js, asn.min.js, and asn-meta.min.js", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "bgp-mini-js-test-"));
  try {
    const v6Start = (0x2606n << 112n) | (0x4700n << 96n);
    const v6End = v6Start | ((1n << 96n) - 1n);
    const v6SubStart = v6Start | (0x4700n << 80n);
    const v6SubEnd = v6SubStart | ((1n << 80n) - 1n);

    fs.writeFileSync(
      path.join(tmp, "autnums.csv"),
      [
        "AS13335,arin,US,CLOUDFLARENET,Cloudflare Inc.,ASSIGNED,,,ARIN,ARIN-HM,abuse@cloudflare.com,Cloudflare CDN",
        "AS64512,ripe,DE,PRIV-NET,Private Org,ASSIGNED,,,,RIPE,,",
        "AS65300,ripe,IR,TEHRAN-NET,Tehran Org,ASSIGNED,,,,RIPE,,",
      ].join("\n") + "\n"
    );
    fs.writeFileSync(
      path.join(tmp, "allocations.csv"),
      [
        `${padV4("1.0.0.0")},${padV4("1.255.255.255")},4,apnic,AU,apnic-net,,,,,,,,,`,
        `${padV4("8.8.8.0")},${padV4("8.8.8.255")},4,arin,US,google-dns,,,,,,,,,`,
        `${padV6(v6Start)},${padV6(v6End)},6,arin,US,cf-v6,,,,,,,,,`,
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
        `${padV4("1.1.1.0")},${padV4("1.1.1.255")},4,1.1.1.0/24,AS13335,apnic,,,,,`,
        `${padV4("1.1.2.0")},${padV4("1.1.2.255")},4,1.1.2.0/24,AS13335,apnic,,,,,`,
        `${padV4("1.1.2.0")},${padV4("1.1.2.255")},4,1.1.2.0/24,AS64512,ripe,,,,,`,
        `${padV4("1.1.2.128")},${padV4("1.1.2.255")},4,1.1.2.128/25,AS65300,ripe,,,,,`,
        `${padV6(v6Start)},${padV6(v6End)},6,2606:4700::/32,AS13335,arin,,,,,`,
        `${padV6(v6SubStart)},${padV6(v6SubEnd)},6,2606:4700:4700::/48,AS64512,ripe,,,,,`,
      ].join("\n") + "\n"
    );

    const countryPath = path.join(tmp, "country.mncc");
    const litePath = path.join(tmp, "bgp-lite.mnl");
    const outDir = path.join(tmp, "dist");

    execFileSync(
      "cargo",
      ["run", "--quiet", "-p", "bgp-tools", "--bin", "build-country-db", "--", "-input", tmp, "-output", countryPath],
      { cwd: repoRoot, env: { ...process.env, TMPDIR: "/var/tmp" } }
    );
    execFileSync(
      "cargo",
      [
        "run", "--quiet", "-p", "bgp-tools", "--bin", "build-lite-db", "--",
        "-input", tmp, "-output", litePath,
        "-release-tag", "db-mini-test", "-built-at", "2026-11-19T00:00:00Z", "-source-commit", "cafebabe",
      ],
      { cwd: repoRoot, env: { ...process.env, TMPDIR: "/var/tmp" } }
    );

    const stats = await buildMiniJsBundles({ countryPath, litePath, outDir });
    assert.equal(stats.length, 3);
    for (const s of stats) {
      assert.ok(s.bundleBytes > 500, `${s.name} should not be empty`);
    }

    const countryMod = (await import(pathToFileURL(path.join(outDir, "country.min.js")).href)) as {
      lookupCountry: (ip: string) => Promise<string | null>;
    };
    const asnMod = (await import(pathToFileURL(path.join(outDir, "asn.min.js")).href)) as {
      lookupAsn: (ip: string) => Promise<AsnLookup | null>;
      getMeta: () => Promise<Record<string, string | null>>;
    };
    const metaMod = (await import(pathToFileURL(path.join(outDir, "asn-meta.min.js")).href)) as {
      lookupAsnMeta: (asn: string | number) => Promise<AsnMeta | null>;
      getMeta: () => Promise<Record<string, string | null>>;
    };

    // 1. Country resolution (RIR + RFC 8805 geofeed override)
    assert.equal(await countryMod.lookupCountry("1.0.5.5"), "AU");
    assert.equal(await countryMod.lookupCountry("1.1.1.1"), "US");
    assert.equal(await countryMod.lookupCountry("::ffff:1.1.1.1"), "US");
    assert.equal(await countryMod.lookupCountry("2606:4700:4700::1"), "GB");
    assert.equal(await countryMod.lookupCountry("9.9.9.9"), null);
    assert.equal(await countryMod.lookupCountry("not-an-ip"), null);

    // 2. IP -> ASN resolution + Chained ASN -> Metadata resolution
    const r1 = await asnMod.lookupAsn("1.1.1.1");
    assert.ok(r1);
    assert.equal(r1.cidr, "1.1.1.0/24");
    assert.equal(r1.asn, "AS13335");
    assert.equal(r1.as_number, 13335);
    assert.deepEqual(r1.asns, ["AS13335"]);
    assert.deepEqual(r1.as_numbers, [13335]);

    const m1 = await metaMod.lookupAsnMeta(r1.as_number!);
    assert.deepEqual(m1, {
      asn: "AS13335",
      as_number: 13335,
      name: "CLOUDFLARENET",
      organization: "Cloudflare Inc.",
      registry: "arin",
      country_code: "US",
    });
    assert.deepEqual(await metaMod.lookupAsnMeta("as13335"), m1);

    // Multi-origin prefix
    const rMulti = await asnMod.lookupAsn("1.1.2.10");
    assert.ok(rMulti);
    assert.equal(rMulti.cidr, "1.1.2.0/24");
    assert.equal(rMulti.asn, null);
    assert.equal(rMulti.as_number, null);
    assert.deepEqual(rMulti.asns, ["AS13335", "AS64512"]);
    assert.deepEqual(rMulti.as_numbers, [13335, 64512]);

    // Sub-/24 unaligned prefix
    const rSub = await asnMod.lookupAsn("1.1.2.200");
    assert.ok(rSub);
    assert.equal(rSub.cidr, "1.1.2.128/25");
    assert.equal(rSub.as_number, 65300);
    const mSub = await metaMod.lookupAsnMeta(rSub.as_number!);
    assert.equal(mSub?.name, "TEHRAN-NET");
    assert.equal(mSub?.country_code, "IR");

    // IPv6 route -> ASN -> Meta
    const r6 = await asnMod.lookupAsn("2606:4700:4700::1");
    assert.ok(r6);
    assert.equal(r6.cidr, "2606:4700:4700::/48");
    assert.equal(r6.as_number, 64512);

    assert.equal(await asnMod.lookupAsn("9.9.9.9"), null);
    assert.equal(await metaMod.lookupAsnMeta(999999), null);
    assert.equal((await asnMod.getMeta()).release_tag, "db-mini-test");
    assert.equal((await metaMod.getMeta()).release_tag, "db-mini-test");
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
