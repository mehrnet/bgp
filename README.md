# MehrNet BGP API

Read-only IP intelligence backed by an immutable, zero-copy memory-mapped (`MNMF`) dataset.
It combines RIR allocations, BGP routes, ASN records, and geofeed locations in one API.

- Full API: [`bgp-api.mehrnet.com`](https://bgp-api.mehrnet.com)
- Serverless Lite API: [`bgp-lite.mehrnet.com`](https://bgp-lite.mehrnet.com)
- Web interface: [`bgp.mehrnet.com`](https://bgp.mehrnet.com)
- Response contract: [`docs/data-contract.md`](docs/data-contract.md)
- OpenAPI: [`openapi.json`](https://bgp.mehrnet.com/openapi.json)

The server only reads the immutable release file. It does not build indexes or call
RIR, RDAP, routing, or geolocation services while handling a request.

## Public Endpoints

| Capability | Full API (`https://bgp-api.mehrnet.com`) | Lite API (`https://bgp-lite.mehrnet.com`) |
| :--- | :--- | :--- |
| Runtime | Bare-metal Rust + zero-copy `mmap` (`MNMF`) | Standalone Cloudflare Worker + `bgp-lite.mnl` (~49 MiB) |
| `GET /v1/health` | Yes (`database: "mmap"`) | Yes (`mode: "lite"`, `database: "lite"`) |
| `GET /v1/ip?query=:ip` | Full IP, route, allocation, geofeed (`country_code`, `region`, `city`, `latitude`, `longitude`), WHOIS (`netname`, `abuse_contact`, dates) | IP, route (`cidr`, `asns`, `origins`), allocation (`registry`, `country_code`, `/32` v4 & `/64` v6), geofeed (`country_code`, `region`, `city`, `latitude`, `longitude`); omits WHOIS prose & `details=full` |
| `GET /v1/country?query=:ip` | — | Yes (plain text or `?format=json`) |
| `GET /v1/asn?query=:asn` | Full `aut-num` + paginated route objects with IRR metadata | `aut-num` (`name`, `organization`, `registry`, `country_code`, `status`, `source`, `maintainers`, `abuse_contact`, `description`) + paginated route prefixes |
| `GET /v1/prefix?prefix=:cidr` | Covering allocation + all overlapping IRR route objects | Covering allocation + winning route objects |
| `GET /v1/range?start=:ip&end=:ip` | Broad `/0`–`/16` summaries & range overlap scans | Not supported (`404`) |

## Install

### Requirements & Recommended Hardware

- **OS & privilege:** Linux with `systemd`, `amd64` or `arm64`, root access (no Rust toolchain required for release installs).

Because `bgp-api` maps the immutable `MNMF` database (~`4.1 GiB` uncompressed) directly via `mmap` with `MADV_RANDOM`, cold uncached lookups fault 4 KiB pages straight from the Linux page cache or disk:

| Tier | CPU | RAM | Disk (NVMe/SSD) | Runtime profile (`BGP_API_CACHE_STRATEGY=auto`) |
| --- | --- | --- | --- | --- |
| **Minimum** | 2 vCPUs | **2–4 GiB** | **12+ GiB free** | `minimal` (`< ~2.3 GiB` RAM) or `balanced` (**4 GiB RAM**: keeps `~1.5 GiB` LPM selectors + `320 MiB` response caches resident; cold records fault from SSD) |
| **Recommended (Production)** | **4+ vCPUs** | **8+ GiB** | **20+ GiB free** | `full` (`>= dataset + 160 MiB caches + 768 MiB reserve ≈ 5.0 GiB`: warms the entire `mmap` file in Linux page cache for zero-disk-I/O `details=full` / range scans and leaves headroom for blue-green staging) |

- **Disk note for updates:** Blue-green updates temporarily hold the active `.mmap`, the compressed download (`~0.9 GiB`), and the staged `.mmap` slot (~`9+ GiB` combined) before deleting the old slot. Hosts with tighter free space automatically fall back to stop-and-replace.
- **Allocator:** Static `amd64` musl release binaries link `tikv-jemallocator` for lock-free multi-threaded allocation; removing `GOMAXPROCS=2` lets Tokio scale across all host vCPUs.

### Local service

Bare-metal installation binds the API to `127.0.0.1:3102`:

```sh
curl -fsSL https://raw.githubusercontent.com/mehrnet/bgp-api/main/install.sh | sudo bash
```

### HTTPS with Caddy

Create a DNS record for the server, then pass the hostname. Caddy is installed and
managed automatically:

```sh
curl -fsSL https://raw.githubusercontent.com/mehrnet/bgp-api/main/install.sh | \
  sudo bash -s -- --domain bgp-api.example.com
```

Add `--auto-update` to schedule a verified release check at **06:00 UTC**:

```sh
curl -fsSL https://raw.githubusercontent.com/mehrnet/bgp-api/main/install.sh | \
  sudo bash -s -- --domain bgp-api.example.com --auto-update
```

The installer downloads the matching static API binary and `mmap` dataset from the
latest GitHub release. It verifies both with `SHA256SUMS.txt` before starting the service.

A public installation stores its hostname in a root-only reinstallation record. After
`--uninstall`, running the normal install command again restores that hostname and its
Caddy vhost automatically; a fresh origin token is generated when needed. The record
never contains the token or dataset. Use `--local-only` to intentionally install only
the loopback service instead.

## Operations

### Check status

```sh
systemctl status bgp-api.service
curl http://127.0.0.1:3102/v1/health
```

Domain deployments serve the active slot through Caddy. The active listener is recorded
in `/var/lib/bgp-api/active-slot` (`primary` on port `3102`, `secondary` on port `3103`).

### Update

```sh
sudo /srv/bgp-api/install.sh --update
```

Updates use the inactive slot when Caddy is configured:

1. Check the latest release tag, publish timestamp, and binary/dataset asset digests (if only the binary changed or `/usr/local/bin/bgp-api` differs, update and restart the active slot in place without re-downloading the database archive).
2. Download and verify the database and binary archives against `SHA256SUMS.txt`.
3. Validate the extracted `.mmap` file before touching active traffic.
4. Release optional response caches in the active slot (`SIGUSR2`) if `MemAvailable` is constrained.
5. Stage blocking cache warmup for the inactive slot via a transient `/run/systemd/system/<service>.d/` drop-in (without mutating `/etc/bgp-api/*.env`), then run `/v1/health` against the staged listener.
6. Atomically move Caddy to the warmed slot, retaining the old slot as a retry target during its drain grace period, then stop and delete the old slot (rolling back via `rollback_staged_slot` on any failure).

This keeps planned dataset updates available without rebuilding indexes on the server.
If Caddy is unavailable, disk space is insufficient, or `BGP_API_BLUE_GREEN=0` is set,
the updater falls back to stop-and-replace mode.

Before staging, the updater checks `MemAvailable` and signals the active slot (`SIGUSR2`) to shed optional response caches if memory is tight while continuing to serve from `mmap`. Kernel page cache is left to Linux to reclaim (never `drop_caches`). Downloads and extraction run at `ionice -c3 nice -n 19`, and the staged slot warms at minimum CPU/I/O weight.

### Logs and scheduled updates

```sh
journalctl -u bgp-api -f
journalctl -u bgp-api-sync --since today
```

### Benchmark production paths

The benchmark keeps connections alive and measures the private loopback origin, direct-origin
HTTPS, and public HTTPS through Cloudflare from an independent host. Direct-origin HTTPS keeps
the hostname for TLS SNI but connects to `BGP_API_ORIGIN_IP`, bypassing Cloudflare only for the
measurement. It is a development operation, not a runtime dependency; Rust (`cargo`) is needed only
on the machine that runs the script.

```sh
BGP_API_ORIGIN_HOST=root@origin.example.com \
BGP_API_ORIGIN_IP=198.51.100.10 \
BGP_API_EXTERNAL_HOST=root@client.example.com \
./scripts/benchmark-production.sh
```

It warms the compact IP cache before each measurement. Override `BGP_API_BENCH_REQUESTS`,
`BGP_API_BENCH_CONCURRENCY`, or `BGP_API_BENCH_WARMUP` to change the workload.

For a manually managed cron schedule:

```cron
CRON_TZ=UTC
0 6 * * * /usr/bin/systemctl start bgp-api-sync.service
```

### Uninstall

The command removes the API services, databases, update schedule, binary, and managed
Caddy configuration. Shared packages such as Caddy remain installed. It retains only
the prior public hostname, so a normal subsequent install restores the Caddy deployment
without requiring `--domain` again.

```sh
sudo /srv/bgp-api/install.sh --uninstall
```

To remove that reinstallation record as well, use:

```sh
sudo /srv/bgp-api/install.sh --uninstall --purge
```

If the local installer is unavailable:

```sh
curl -fsSL https://raw.githubusercontent.com/mehrnet/bgp-api/main/install.sh | \
  sudo bash -s -- --uninstall
```

## API

All endpoints are `GET` requests and return JSON.

| Endpoint | Use |
| --- | --- |
| `/v1/ip?query=1.1.1.1` | Most-specific route, allocation, and location |
| `/v1/ip?query=1.1.1.1&details=full` | IP lookup with matching source records |
| `/v1/prefix?prefix=1.1.1.0/24` | Allocation and route records for a CIDR |
| `/v1/range?start=1.1.1.0&end=1.1.1.255` | Records overlapping an inclusive range |
| `/v1/asn?query=AS13335` | ASN identity and registered routes |
| `/v1/health` | Runtime and dataset metadata |

Paginated endpoints accept:

| Parameter | Applies to | Notes |
| --- | --- | --- |
| `limit` | prefix, range, ASN | `1`-`100`; default is server-defined |
| `cursor` | prefix, ASN | Continue the previous result set |
| `cursor` | range | Opaque cursor returned by the preceding range response; pass it unchanged |
| `page` | ASN | Numbered pagination; do not combine with `cursor` |
| `details=full` | IP | Include matching allocation, route, and geofeed records |
| `kind=allocations\|routes` | range | Select range record types |

Canonical IPv4 `/0` through `/16` range requests use one precomputed summary per
prefix. Narrow ranges, IPv6 ranges, and non-CIDR ranges return source records with
pagination.

Example:

```sh
curl -sS 'https://bgp-api.mehrnet.com/v1/ip?query=1.1.1.1' | jq
curl -sS 'https://bgp-api.mehrnet.com/v1/asn?query=AS13335&limit=20' | jq
```

See [`docs/data-contract.md`](docs/data-contract.md) for stable response objects and
the public [OpenAPI document](https://bgp.mehrnet.com/openapi.json) for generated
client documentation.

## Configuration

The installer writes `/etc/bgp-api/bgp-api.env`:

| Variable | Default | Purpose |
| --- | --- | --- |
| `BGP_API_DATABASE_PATH` | `/var/lib/bgp-api/primary.mmap` | Active database path |
| `BGP_API_PRIMARY_DATABASE_PATH` | `/var/lib/bgp-api/primary.mmap` | Primary update slot |
| `BGP_API_SECONDARY_DATABASE_PATH` | `/var/lib/bgp-api/secondary.mmap` | Secondary update slot |
| `LISTEN_ADDR` | `127.0.0.1:3102` | HTTP listener for this service |
| `BGP_API_BLUE_GREEN` | `auto` | `auto`, `1`, or `0` |
| `BGP_API_STARTUP_TIMEOUT_SECONDS` | `300` | Maximum wait for a staged API slot to become healthy |
| `BGP_API_CACHE_STRATEGY` | `auto` | `auto`, `minimal`, `balanced`, or `full` |
| `BGP_API_DEFER_CACHE_WARMUP` | `0` | Start serving before cache warmup |
| `BGP_API_BLOCK_UNTIL_CACHE_WARMUP` | `0` | Finish the selected warmup before opening the listener (set transiently by the updater via `/run/systemd/system/` drop-in) |
| `BGP_API_RUNTIME_CACHE_CONTROL` | `1` | Enable `SIGUSR2` release of optional in-process caches for a memory-constrained staged update |
| `BGP_API_STAGE_MEMORY_RESERVE_MIB` | `768` | Minimum `MemAvailable` before the updater asks the active slot to release optional caches |
| `BGP_API_DRAIN_GRACE_SECONDS` | `45` | Keep the old backend running after the Caddy switch so stale proxy connections can finish |
| `CORS_ALLOWED_ORIGINS_JSON` | empty | JSON array of browser origins |
| `ORIGIN_AUTH_TOKEN` | empty | Proxy-to-origin authentication token |
| `BGP_API_COMPACT_CACHE_MIB` | strategy default | Override the 16-shard in-process `RwLock` cache budget for default IP responses |
| `BGP_API_RESOURCE_CACHE_MIB` | strategy default | Override the 16-shard in-process `RwLock` cache budget for prefix, range, and ASN responses |
| `GOMAXPROCS` | host CPU count | Worker threads override (`1`–`256`); omitted by `install.sh` so Tokio uses all host vCPUs |

`auto` selects the safest profile from the deployed `mmap` file size and `MemTotal`:

| Profile | Threshold & runtime behavior |
| --- | --- |
| `minimal` | Fallback when below `balanced` threshold: `64 MiB` compact cache, `16 MiB` resource cache, no preload |
| `balanced` | Requires `selectors + 320 MiB caches + 512 MiB reserve`: `256 MiB` compact cache, `64 MiB` resource cache, warms the LPM selector slices in the kernel page cache |
| `full` | Requires `database + 160 MiB caches + 768 MiB reserve`: `128 MiB` compact cache, `32 MiB` resource cache, then a sequential warm of the immutable `mmap` file through the Linux page cache |

## Releases and datasets

The daily GitHub Actions workflow runs at 04:00 UTC and supports manual dispatch. RIR
parsers run in a matrix; release formats and static binaries are built independently.

| Asset | Purpose |
| --- | --- |
| `mehrnet_bgp_mmap.tar.zst` | Ready-to-query production dataset |
| `mehrnet_bgp_sqlite.tar.zst` | Optional SQLite export for third-party users |
| `bgp-api-linux-amd64.tar.gz` | Static Linux `amd64` binary |
| `bgp-api-linux-arm64.tar.gz` | Static Linux `arm64` binary |
| `SHA256SUMS.txt` | Asset integrity checksums |

The API runtime uses the immutable `mmap` file (`MNMF`). SQLite is a separate export and is not required for serving
requests.

## Build from source

Rust 1.88 or newer is required for local builds (`edition = "2024"`).

Repository layout:

- `crates/mnmf-store`: zero-copy `MNMF` reader and builder
- `crates/bgp-api-core`: shared request routing, response models, pagination, CORS/auth, and caches
- `crates/bgp-api-server`: bare-metal Tokio + Hyper `bgp-api` binary
- `crates/bgp-compact`: `country.mncc` (`MNCC`) and `bgp-lite.mnl` (`MNL1`) readers/builders
- `crates/bgp-sqlite`: SQLite export builder, indexer, summary generator, and validator
- `crates/bgp-tools`: pipeline tools (`parse-rir`, `fetch-geofeeds`, `build-mmap-dataset`, `build-country-db`, `build-lite-db`, `benchmark-http`)
- `lite/`: standalone Cloudflare Worker for `bgp-lite.mehrnet.com`

Run the API against a local dataset:

```sh
export BGP_API_DATABASE_PATH="$PWD/mehrnet_bgp.mmap"
cargo run --release -p bgp-api-server --bin bgp-api
```

Build a static Linux `amd64` musl binary (`musl-tools` required on the build host for `tikv-jemalloc-sys`):

```sh
sudo apt-get install -y musl-tools
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
RUSTFLAGS="-C target-feature=+crt-static" \
cargo build --release --target x86_64-unknown-linux-musl -p bgp-api-server --bin bgp-api
```

Build a `mmap` dataset from producer output:

```sh
cargo run --release -p bgp-tools --bin build-mmap-dataset -- \
  -input final_data \
  -output mehrnet_bgp.mmap \
  -release-tag local \
  -built-at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  -source-commit "$(git rev-parse HEAD)"
```

The optional SQLite export is built from the same `final_data/` directory:

```sh
cargo run --release -p bgp-sqlite --bin build-sqlite-dataset -- \
  -input final_data \
  -output mehrnet_bgp.db \
  -release-tag local \
  -built-at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  -source-commit "$(git rev-parse HEAD)"
cargo run --release -p bgp-sqlite --bin build-prefix-index -- mehrnet_bgp.db
cargo run --release -p bgp-sqlite --bin build-range-summaries -- mehrnet_bgp.db
cargo run --release -p bgp-sqlite --bin validate-database -- -indexed mehrnet_bgp.db
```

## Development

```sh
shellcheck install.sh scripts/*.sh
./scripts/check-quality.sh
npm --prefix lite test
```
