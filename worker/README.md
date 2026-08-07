# Fleet proxy

A browser cannot read GitHub release asset bytes. The blob host they redirect to
sends no `Access-Control-Allow-Origin`, and that is true of every route to them:
the download link, the signed URL behind it, and the REST asset endpoint, whose
302 does carry CORS but lands on a blob that does not.

So the bytes need a hop, and this is it. It fetches the asset server-side,
where CORS does not apply, and re-serves it with the header the browser needs.

```
GET /fleet?tag=firmware-2026-07-30&name=fleet-index.json
GET /fleet?repo=gtxaspec&tag=firmware-2026-07-30&name=fleet-reports.tar.gz
GET /fleet/releases?repo=gtxaspec
```

Discovering *which* releases carry a snapshot goes through here too, and not
for CORS. `api.github.com` would answer that, but its releases endpoint embeds
every release's full asset list: for a firmware repo that is ~18 MB of JSON and
8000 asset objects to learn a handful of tag names, spent out of the reader's
own 60-per-hour unauthenticated quota, so a shared address runs out and the
page silently shows nothing. `/fleet/releases` answers the same question in
tens of bytes, from signals that do not share a quota: the `tags.atom` feed,
~10 KB and outside the API limit entirely; the newest tag from the 302 that
`github.com/<repo>/releases/latest` returns, which needs no parsing either; the
tags endpoint, which reaches further back when it is not rate limited; and one
`HEAD` each to see which actually carry the asset.

Confirmed tags are remembered between refreshes. The API allows 60 requests an
hour *per IP* and a Worker's outbound address is shared with every other
customer on that edge, so it is refused often; when it was the only source of
candidates, a refusal collapsed the list to the single tag the redirect gives,
and that answer was cached like a real one, so a published release quietly
stopped being offered. Remembering means a failed enumeration cannot drop a
release that is still published, and a release stays offered once it has aged
out of the feed. Every remembered tag is re-probed before it is offered, so one
whose assets are deleted still drops away.

Only `fleet-index.json` and `fleet-reports.tar.gz`, only the repos named in
`REPOS`, and a tag has to look like a tag. The repo and the asset name are what
keep this from being an open proxy; the tag needs no prefix rule, since every
release of an allow-listed repo is public anyway, and a branch that publishes
snapshots under its own name -- `master-2026-08-07` beside
`firmware-2026-08-02` -- is then readable without redeploying this. Discovery
is narrower than serving: it only spends a `HEAD` on `firmware-*` and `master-*`
tags, because a repo also tags caches and toolchains that never carry one. That allow-list is the security model: without it
this would be an open proxy anyone could aim at any URL, on someone else's
bandwidth. `repo` exists so a CI change can be proven on the fork before it
lands on the main repo.

This is its own Worker on purpose. Firmware flashing and the image builder are
separate Workers for the same reason -- a bad deploy of one must not be able to
take down the others -- and the free request budget is account-wide, so
splitting costs nothing.

```
npx wrangler deploy     # from worker/
npx wrangler tail
```

Responses are `max-age=300, stale-while-revalidate=86400`. Five minutes fresh
rather than the day a released image would get, because a snapshot is
re-uploaded to the same tag with `--clobber` on every rerun; the long stale
window is safe because upstream's `ETag` and `Last-Modified` are passed
through and `If-None-Match` is answered with a 304. A reader whose copy has
expired revalidates in a few bytes instead of pulling megabytes again, and a
rerun changes the ETag, so freshness is better than a short cache alone would
give.
