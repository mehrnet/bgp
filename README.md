# bgp

Static public interface for MehrNet BGP at `https://bgp.mehrnet.com`.

This repository contains only the browser UI. The API, PostgreSQL producer,
release workflow, and production sync tooling live in
[`mehrnet/bgp-api`](https://github.com/mehrnet/bgp-api).

## Features

- Shows the visitor's current IP information by default.
- Supports direct lookup of IPv4 and IPv6 addresses.
- Supports CIDR prefix lookups, address ranges, ASNs, and text search.
- Links queryable values in the response so a prefix, range, or ASN can be
  opened directly.
- Provides API documentation with examples for curl, JavaScript, PHP, Python,
  and Go.
- Ships a static `openapi.json` file for the public API contract.

The browser temporarily resolves its IPv4 and optional IPv6 addresses through
IPify, then looks each address up through `bgp-api`. The dedicated responders
are maintained separately in [`mehrnet/ip-api`](https://github.com/mehrnet/ip-api)
and will replace IPify after `ipv4.mehrnet.com` and `ipv6.mehrnet.com` are
deployed. A single HTTP request can only arrive over one IP family, so the
separate IPv6 resolution remains necessary for dual-stack visitors.

## Files

- `index.html`: page markup and client-side templates.
- `style.css`: responsive UI styling.
- `app.js`: lookup routing, DOM rendering, docs tabs, and copy buttons.
- `icons.svg`: bundled country flags and UI/language icons.
- `favicon.svg`: MehrNet BGP favicon.
- `openapi.json`: static OpenAPI description for `bgp-api.mehrnet.com`.
- `_headers`: static asset headers for Cloudflare Workers Assets.
- `.assetsignore`: controls the files included in the deployed asset bundle.

## Local Preview

The site is static, so any local static server is enough:

```sh
python3 -m http.server 8787
```

Then open `http://127.0.0.1:8787`.

## Deploy

Deploy with Wrangler using the current Cloudflare credentials:

```sh
npx wrangler deploy
```

`wrangler.toml` publishes the repository root as Cloudflare Workers Assets and
uses single-page app fallback behavior. `.assetsignore` keeps repository
metadata, Wrangler state, and deployment configuration out of the public asset
bundle while including only the required static files.

## API

The frontend expects the production API at:

```text
https://bgp-api.mehrnet.com
```

Useful endpoints:

- `GET /v1/ip?query={address}`
- `GET /v1/ip?query={address}&details=full`
- `GET /v1/prefix?prefix={cidr}`
- `GET /v1/range?start={ip}&end={ip}&kind=allocations|routes`
- `GET /v1/asn?query={asn}`
- `GET /v1/health`

The public API documentation is available at `/api`. Browser routes use
pathname URLs, including `/my-ip`, `/ip/{address}`, `/cidr/{prefix}`,
`/range?start={ip}&end={ip}`, and `/asn/{asn}`.
