# Rust Codebase Standards

This workspace implements the bare-metal MehrNet BGP API runtime reading immutable
`MNMF` (`mehrnet_bgp.mmap`) datasets directly from virtual memory.

## Structural & Size Limits

- **File limit:** Every `.rs` source file in `crates/*/src/` and `crates/*/tests/` must stay **<= 250 lines**. Split cohesive concerns into dedicated submodules early rather than letting files grow monolithic.
- **Function limit:** Every function must stay **<= 50 lines**, **<= 6 arguments**, and cognitive complexity **<= 12** (enforced by `clippy.toml` and `[workspace.lints.clippy]`).
- **No duplication:** Shared binary-search, cursor, IP math, CORS/auth, and string/coordinate normalization logic must live in shared helpers (`mnmf-store::index`, `bgp-compact::city_coords`, `bgp-api-core::ipkey`, `bgp-api-core::handlers`), never copy-pasted across IPv4/IPv6 paths or endpoints.

## Safety & Reliability Rules

- **`unsafe` is denied workspace-wide** (`unsafe_code = "deny"`). Only the single `memmap2::Mmap::map` constructor in `mnmf-store` may opt in with an explicit `// SAFETY:` block documenting why the backing file is immutable.
- **No panics in request or decode paths:** `unwrap_used`, `expect_used`, `panic`, and unchecked `indexing_slicing` (`slice[i]`) are all **`deny`**. Always use `.get(...)`, `?`, and explicit `StoreError` / `ApiError` variants.
- **Zero-copy reads first:** Traverse LPM tries and fixed-stride indexes directly on `&[u8]` inside the mapped file, and decode string fields as `Cow<'a, str>` (`mnmf-store::borrowed`) without heap-copying tables or intermediate strings.

## Mandatory Verification Gate

Before committing any change, run from the repository root:

```sh
shellcheck install.sh scripts/*.sh
./scripts/check-quality.sh
npm --prefix lite test
```

This runs shell script linting, line-count enforcement, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and all Rust and Cloudflare Worker tests.
