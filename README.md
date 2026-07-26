# bgp

Static IP intelligence interface for `https://bgp.mehrnet.com`.

The page looks up the visitor through `https://bgp-api.mehrnet.com/v1/me` and
supports IPv4 and IPv6 lookups through `/v1/ip/:ip`.

## Deploy

```sh
npx wrangler deploy
```

The `wrangler.toml` configuration publishes the repository root as static
Worker assets. `.assetsignore` excludes repository metadata, Wrangler's local
temporary files, and deployment configuration from the public asset bundle.
