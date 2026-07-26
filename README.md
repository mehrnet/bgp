# bgp

Static IP intelligence interface for `https://bgp.mehrnet.com`.

The page looks up the visitor through `https://bgp-api.mehrnet.com/v1/me` and
supports IPv4 and IPv6 lookups through `/v1/ip/:ip`. It also makes separate
browser requests to IPify's public `api4.ipify.org` and `api6.ipify.org`
endpoints so visitors with dual-stack connectivity can see both public
addresses. Selecting either address retrieves its detailed record from the
MehrNet API.

## Deploy

```sh
npx wrangler deploy
```

The `wrangler.toml` configuration publishes the repository root as static
Worker assets. `.assetsignore` excludes repository metadata, Wrangler's local
temporary files, and deployment configuration from the public asset bundle.
