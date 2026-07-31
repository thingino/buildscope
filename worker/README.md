# Fleet proxy

A browser cannot read GitHub release asset bytes. The blob host they redirect to
sends no `Access-Control-Allow-Origin`, and that is true of every route to them:
the download link, the signed URL behind it, and the REST asset endpoint, whose
302 does carry CORS but lands on a blob that does not. Listing releases is fine
without help, because `api.github.com` does send CORS.

So only the bytes need a hop, and this is it. It fetches the asset server-side,
where CORS does not apply, and re-serves it with the header the browser needs.

```
GET /fleet?tag=firmware-2026-07-30&name=fleet-index.json
GET /fleet?repo=gtxaspec&tag=firmware-2026-07-30&name=fleet-reports.tar.gz
```

Only `firmware-*` tags, only `fleet-index.json` and `fleet-reports.tar.gz`, only
the repos named in `REPOS`. That allow-list is the security model: without it
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

The response is cached at the edge for five minutes, not the day a released
image would get: a snapshot is re-uploaded to the same tag with `--clobber` on
every rerun, so caching it hard would serve a stale fleet from a tag that has
since changed.
