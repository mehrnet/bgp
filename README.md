# bgp

Static IP intelligence interface for `https://bgp.mehrnet.com`.

The page looks up the visitor through `https://bgp-api.mehrnet.com/v1/me` and
supports IPv4 and IPv6 lookups through `/v1/ip/:ip`.

## Deploy

```sh
npx wrangler deploy
```

The `wrangler.toml` configuration publishes only `public/` as static Worker
assets and attaches the `bgp.mehrnet.com` custom domain.
