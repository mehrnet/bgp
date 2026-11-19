# MehrBGP

Read-only IP intelligence backed by an immutable, zero-copy memory-mapped (`MNMF`) dataset combining RIR allocations, BGP routes, ASN records, and geofeed locations.

- Full API: [`bgp-api.mehrnet.com`](https://bgp-api.mehrnet.com)
- Lite API: [`bgp-lite.mehrnet.com`](https://bgp-lite.mehrnet.com)
- Web UI: [`bgp.mehrnet.com`](https://bgp.mehrnet.com)
- Contract: [`docs/data-contract.md`](docs/data-contract.md) · OpenAPI: [`openapi.json`](https://bgp.mehrnet.com/openapi.json)

## Endpoints

All endpoints use HTTP `GET` and return JSON (except `/v1/country`, plain text by default).

| Endpoint | Full API (`bgp-api`) | Lite API (`bgp-lite`) | Notes |
| --- | --- | --- | --- |
| `/v1/ip?query=:ip` | Route, allocation, geofeed, WHOIS, `&details=full` | Route, `/32` v4 & `/64` v6 allocs, geofeed | Primary lookup |
| `/v1/prefix?prefix=:cidr` | Allocation + overlapping IRR routes | Allocation + winning routes | Supports `limit`, `cursor` |
| `/v1/range?start=:ip&end=:ip` | `/0`–`/16` summaries + overlap records | `404` (unsupported) | Supports `kind`, `limit`, `cursor` |
| `/v1/asn?query=:asn` | Full `aut-num` + paginated routes | `aut-num` + paginated prefixes | Supports `limit`, `page` or `cursor` |
| `/v1/country?query=:ip` | `404` | ISO country (`?format=json` optional) | Defaults to caller IP |
| `/v1/health` | `database: "mmap"` | `mode: "lite"`, `database: "lite"` | Dataset & build metadata |

```sh
curl -sS 'https://bgp-api.mehrnet.com/v1/ip?query=1.1.1.1' | jq
curl -sS 'https://bgp-api.mehrnet.com/v1/asn?query=AS13335&limit=20' | jq
```

## Static JS Bundles (jsDelivr CDN)

Self-contained ES modules are built and force-pushed as a single-commit snapshot to the `mini-js` orphan branch on every dataset release:

| Bundle (`https://cdn.jsdelivr.net/gh/mehrnet/bgp@mini-js/...`) | Export | Resolution |
| --- | --- | --- |
| `country.min.js` | `lookupCountry(ip)` | IP -> ISO 3166-1 alpha-2 country code (`"US"`, `"DE"`, ...) |
| `asn.min.js` | `lookupAsn(ip)` | IP -> `{ cidr, asn, as_number, asns, as_numbers }` |
| `asn-meta.min.js` | `lookupAsnMeta(asn)` | ASN (`13335` or `"AS13335"`) -> `{ asn, as_number, name, organization, registry, country_code }` |

```js
import { lookupCountry } from "https://cdn.jsdelivr.net/gh/mehrnet/bgp@mini-js/country.min.js";
import { lookupAsn } from "https://cdn.jsdelivr.net/gh/mehrnet/bgp@mini-js/asn.min.js";
import { lookupAsnMeta } from "https://cdn.jsdelivr.net/gh/mehrnet/bgp@mini-js/asn-meta.min.js";

const country = await lookupCountry("1.1.1.1");       // "US"
const route = await lookupAsn("1.1.1.1");             // { cidr: "1.1.1.0/24", asn: "AS13335", as_number: 13335, ... }
const meta = await lookupAsnMeta(route.as_number);    // { asn: "AS13335", name: "CLOUDFLARENET", organization: "...", registry: "arin", country_code: "US" }
```

## Hardware & Requirements

Linux with `systemd`, `amd64` or `arm64`, and root access (no Rust toolchain needed for release installs). Because `bgp-api` maps the `~4.1 GiB` `MNMF` dataset with `MADV_RANDOM`, cold lookups fault from page cache or disk:

| Tier | CPU | RAM | Disk (NVMe/SSD) | Default profile |
| --- | --- | --- | --- | --- |
| Minimum | 2 vCPUs | 2–4 GiB | 12+ GiB free | `minimal` / `balanced` (cold faults from SSD; stop-and-replace if disk is tight) |
| Recommended | 4+ vCPUs | 8+ GiB | 20+ GiB free | `full` (keeps entire `mmap` in page cache; headroom for blue-green updates) |

## Install & Operations

```sh
# Loopback only (127.0.0.1:3102)
curl -fsSL https://raw.githubusercontent.com/mehrnet/bgp/main/install.sh | sudo bash

# Public HTTPS with managed Caddy (+ optional daily 06:00 UTC auto-update)
curl -fsSL https://raw.githubusercontent.com/mehrnet/bgp/main/install.sh | \
  sudo bash -s -- --domain bgp-api.example.com --auto-update

# Status, update, logs, uninstall
systemctl status bgp-api.service
sudo /srv/bgp-api/install.sh --update
journalctl -u bgp-api -f
sudo /srv/bgp-api/install.sh --uninstall        # add --purge to clear saved domain state
```

## Configuration

Configured via `/etc/bgp-api/bgp-api.env`:

| Variable | Default | Purpose |
| --- | --- | --- |
| `BGP_API_DATABASE_PATH` | `/var/lib/bgp-api/primary.mmap` | Active dataset path (`primary`/`secondary` slots) |
| `LISTEN_ADDR` | `127.0.0.1:3102` | Bind address |
| `BGP_API_CACHE_STRATEGY` | `auto` | `auto`, `minimal`, `balanced`, or `full` (`mmap` page-cache warmup) |
| `BGP_API_BLUE_GREEN` | `auto` | `auto`, `1`, or `0` (staged slot handoff with Caddy) |
| `BGP_API_COMPACT_CACHE_MIB` | strategy default | Override compact IP response cache budget |
| `BGP_API_RESOURCE_CACHE_MIB` | strategy default | Override prefix/range/ASN response cache budget |
| `BGP_API_RUNTIME_CACHE_CONTROL` | `1` | Release optional caches on `SIGUSR2` under memory pressure |
| `CORS_ALLOWED_ORIGINS_JSON` | empty | JSON array of allowed browser origins |
| `ORIGIN_AUTH_TOKEN` | empty | Shared proxy-to-origin token |
| `GOMAXPROCS` | host CPUs | Tokio worker thread count override (`1`–`256`) |

## Architecture

- `crates/mnmf-store`: zero-copy `MNMF` reader and builder
- `crates/bgp-api-core`: shared routing, models, pagination, CORS/auth, and caches
- `crates/bgp-api-server`: bare-metal Tokio + Hyper `bgp-api` binary (`tikv-jemallocator` on musl)
- `crates/bgp-compact`: `country.mncc` (`MNCC`) and `bgp-lite.mnl` (`MNL1`) readers/builders
- `crates/bgp-sqlite`: optional SQLite export pipeline
- `crates/bgp-tools`: data parsers, builders, and `benchmark-http`
- `lite/`: standalone Cloudflare Worker for `bgp-lite.mehrnet.com`
- `mini-js/`: static browser ESM bundle generator (`country.min.js`, `asn.min.js`, `asn-meta.min.js`)
- `web/`: SolidJS + TypeScript web frontend for `bgp.mehrnet.com`

## Development

Rust 1.88+ (`edition = "2024"`) and Node.js 24+:

```sh
shellcheck install.sh scripts/*.sh
./scripts/check-quality.sh
npm --prefix lite run typecheck && npm --prefix lite test
npm --prefix mini-js run typecheck && npm --prefix mini-js test
npm --prefix web run check && npm --prefix web test && npm --prefix web run build
```

Production benchmark (from an independent client host):

```sh
BGP_API_ORIGIN_HOST=root@origin.example.com BGP_API_ORIGIN_IP=198.51.100.10 \
  BGP_API_EXTERNAL_HOST=root@client.example.com ./scripts/benchmark-production.sh
```
