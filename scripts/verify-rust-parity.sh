#!/usr/bin/env bash
set -Eeuo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMPDIR="${TMPDIR:-/var/tmp}"
export TMPDIR
work_dir="$(mktemp -d "$TMPDIR/rust-parity.XXXXXX")"
go_pid=""
rs_pid=""
cleanup() {
  [ -n "$go_pid" ] && kill "$go_pid" 2>/dev/null || true
  [ -n "$rs_pid" ] && kill "$rs_pid" 2>/dev/null || true
  wait 2>/dev/null || true
  rm -rf -- "$work_dir"
}
trap cleanup EXIT

mkdir -p "$work_dir/csv"
python3 - <<'PY' "$work_dir/csv"
import csv, ipaddress, os, sys

out = sys.argv[1]
def k(ip_str):
    addr = ipaddress.ip_address(ip_str)
    if addr.version == 4:
        return str((0xffff << 32) | int(addr))
    return str(int(addr))

with open(os.path.join(out, "allocations.csv"), "w", newline="") as f:
    w = csv.writer(f)
    w.writerow([k("1.0.0.0"), k("1.255.255.255"), "4", "APNIC", "AU", "APNIC-1", "ALLOCATED PORTABLE", "20110811", "", "", "APNIC", "", "", "", ""])
    w.writerow([k("1.1.1.0"), k("1.1.1.255"), "4", "APNIC", "AU", "APNIC-LABS", "ALLOCATED PORTABLE", "20110811", "", "", "APNIC", "", "", "abuse@example.test", ""])
    w.writerow([k("2001:db8::"), k("2001:db8:ffff:ffff:ffff:ffff:ffff:ffff"), "6", "ARIN", "US", "V6-DOC", "ALLOCATED", "20200101", "", "", "ARIN", "", "", "v6@example.test", ""])
with open(os.path.join(out, "allocations.csv"), "ab") as f:
    f.write(f"{k('2.2.2.0')},{k('2.2.2.255')},4,ripe,DE,WILHELM,ALLOCATED,20110811,,,ripe,,Org\xdf,,Heidbergstra\xdfe\n".encode("latin-1"))

with open(os.path.join(out, "routes.csv"), "w", newline="") as f:
    w = csv.writer(f)
    w.writerow([k("1.1.1.0"), k("1.1.1.255"), "4", "1.1.1.0/24", "AS13335", "APNIC", "APNIC", "", "", "abuse@example.test", ""])
    w.writerow([k("1.1.1.0"), k("1.1.1.255"), "4", "1.1.1.0/24", "AS64500", "APNIC", "APNIC", "", "", "", ""])
    w.writerow([k("2001:db8::"), k("2001:db8:ffff:ffff:ffff:ffff:ffff:ffff"), "6", "2001:db8::/32", "AS13335", "ARIN", "ARIN", "", "", "", ""])

with open(os.path.join(out, "geolocations.csv"), "w", newline="") as f:
    w = csv.writer(f)
    w.writerow([k("1.0.0.0"), k("1.255.255.255"), "4", "AU", "New South Wales", "Sydney"])
    w.writerow([k("1.1.1.0"), k("1.1.1.255"), "4", "AU", "Queensland", "South Brisbane"])

with open(os.path.join(out, "autnums.csv"), "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["AS13335", "APNIC", "US", "CLOUDFLARENET", "ORG-CLOUD14-ARIN", "ASSIGNED", "", "", "APNIC", "", "abuse@example.test", ""])
PY

go run ./cmd/build-mmap-dataset \
  -input "$work_dir/csv" \
  -output "$work_dir/test.mmap" \
  -release-tag db-parity \
  -built-at 2026-11-17T00:00:00Z \
  -source-commit abcdef >/dev/null

go build -o "$work_dir/bgp-api-go" ./cmd/bgp-api
cargo build --manifest-path rust/Cargo.toml --bin bgp-api
rs_bin="$ROOT_DIR/rust/target/debug/bgp-api"

BGP_API_DATABASE_PATH="$work_dir/test.mmap" BGP_API_VALIDATE_ONLY=1 "$rs_bin"

common_env=(
  "BGP_API_DATABASE_PATH=$work_dir/test.mmap"
  "BGP_API_CACHE_STRATEGY=minimal"
  "BGP_API_RUNTIME_CACHE_CONTROL=1"
  "ORIGIN_AUTH_TOKEN=parity-secret"
  "CORS_ALLOWED_ORIGINS_JSON=[\"https://bgp.mehrnet.com\"]"
)

env "${common_env[@]}" LISTEN_ADDR=127.0.0.1:18102 "$work_dir/bgp-api-go" >"$work_dir/go.log" 2>&1 &
go_pid=$!
env "${common_env[@]}" LISTEN_ADDR=127.0.0.1:18103 "$rs_bin" >"$work_dir/rs.log" 2>&1 &
rs_pid=$!

for port in 18102 18103; do
  ready=0
  for _ in $(seq 1 50); do
    if curl -fsS -H "X-BGP-API-Origin-Token: parity-secret" "http://127.0.0.1:${port}/v1/health" >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 0.1
  done
  [ "$ready" -eq 1 ] || { echo "server on port $port did not start"; exit 1; }
done

python3 - <<'PY'
import json, urllib.request, urllib.error

TOKEN = {"X-BGP-API-Origin-Token": "parity-secret", "Origin": "https://bgp.mehrnet.com"}

def fetch(port, path, headers=TOKEN, method="GET"):
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            return r.status, {k.lower(): v for k, v in r.headers.items()}, r.read().decode("utf-8")
    except urllib.error.HTTPError as e:
        return e.code, {k.lower(): v for k, v in e.headers.items()}, e.read().decode("utf-8")

def compare(path, headers=TOKEN, method="GET"):
    s1, h1, b1 = fetch(18102, path, headers, method)
    s2, h2, b2 = fetch(18103, path, headers, method)
    assert s1 == s2, f"{path}: status {s1} != {s2}"
    assert h1.get("access-control-allow-origin") == h2.get("access-control-allow-origin"), f"{path}: CORS mismatch {h1} vs {h2}"
    if not b1 and not b2:
        return {}
    j1, j2 = json.loads(b1), json.loads(b2)
    assert j1 == j2, f"{path}: JSON mismatch\nGO={j1}\nRS={j2}"
    return j2

compare("/")
compare("/v1/health")
compare("/v1/health", headers={})
compare("/v1/ip?query=1.1.1.1", method="OPTIONS")
compare("/v1/ip?query=1.1.1.1")
compare("/v1/ip?query=1.1.1.1&details=full")
compare("/v1/ip?query=2.2.2.1&details=full")
compare("/v1/ip?query=2001:db8::1")
compare("/v1/ip?query=8.8.8.8")
compare("/v1/ip?query=bad-ip")
compare("/v1/ip?query=1.1.1.1&details=bogus")
compare("/v1/prefix?prefix=1.1.1.0/24&limit=1")
compare("/v1/prefix?prefix=bad")

r1 = compare("/v1/range?start=1.1.1.0&end=1.1.1.255&kind=allocations&limit=1")
c1 = r1["next_cursor"]
assert c1, "missing covering cursor"
compare(f"/v1/range?start=1.1.1.0&end=1.1.1.255&kind=allocations&limit=1&cursor={c1}")
compare(f"/v1/range?start=1.1.1.0&end=1.1.1.255&kind=routes&limit=1&cursor={c1}")
compare("/v1/range?start=1.0.0.0&end=1.255.255.255")
compare("/v1/range?start=1.0.0.0&end=1.31.255.255")

a1 = compare("/v1/asn?query=AS13335&limit=1")
assert a1["routes"]["next_cursor"], "missing asn cursor"
compare(f"/v1/asn?query=13335&limit=1&cursor={a1['routes']['next_cursor']}")
compare("/v1/asn?query=AS13335&page=1&limit=1")
compare("/v1/asn?query=AS64599")
compare("/v1/asn?query=AS0")
compare("/v1/asn?query=bad")
compare("/v1/asn?query=13335&page=1&cursor=1")
compare("/unknown")
print("Go vs Rust differential parity OK (24 checks passed cleanly).")
PY
