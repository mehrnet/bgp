# bgp

Static IP intelligence interface for `https://bgp.mehrnet.com`.

The page looks up the visitor through `https://bgp-api.mehrnet.com/v1/me` and
supports IPv4/IPv6 addresses, CIDRs, address ranges (`start - end`), and ASNs.
`/v1/me` is the
authoritative current-connection lookup. A separate browser request to
IPify's public `api6.ipify.org` endpoint is used only to discover an
additional IPv6 address for dual-stack visitors; selecting it retrieves its
detailed record from the MehrNet API.

## Deploy

```sh
npx wrangler deploy
```

The `wrangler.toml` configuration publishes the repository root as static
Worker assets. `.assetsignore` excludes repository metadata, Wrangler's local
temporary files, and deployment configuration from the public asset bundle.
