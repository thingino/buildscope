/**
 * buildscope fleet proxy.
 *
 * GitHub serves release assets from a blob host that sends no
 * Access-Control-Allow-Origin, so a browser cannot read their bytes: not from the
 * github.com download link, not from the signed URL it redirects to, and not via
 * the REST asset endpoint (whose 302 does carry CORS, but redirects to a blob that
 * does not). Listing releases needs no help - api.github.com sends CORS - so only
 * the bytes need this hop.
 *
 * This Worker is that hop, for the two assets a fleet snapshot publishes. It is
 * deliberately its own Worker rather than a route added to an existing one: a
 * bad deploy here must not be able to take down firmware flashing or the image
 * builder, and the free request budget is account-wide so splitting costs
 * nothing. The body is streamed straight through, never buffered.
 *
 *   GET /fleet?tag=firmware-2026-07-30&name=fleet-index.json
 *   GET /fleet?repo=gtxaspec&tag=...&name=fleet-reports.tar.gz
 *   GET /fleet/releases?repo=gtxaspec   -> which releases carry a snapshot
 */

/* Which repos may be read, by short name. An allow-list rather than a free
 * parameter: a caller-supplied repo would make this an open proxy pointed at
 * any URL on someone else's bandwidth. The fork is here so a CI change can be
 * proven on it before it lands on the main repo. */
const REPOS = {
    thingino: 'themactep/thingino-firmware',
    gtxaspec: 'gtxaspec/thingino-firmware',
};
const DEFAULT_REPO = 'thingino';

/* Bounds the HEADs per refresh. Each is one HEAD and the answer is cached, so
 * this is cheap. Covers remembered tags as well as freshly
 * discovered ones, so it is the length of the history the picker can offer. */
const PROBE_LIMIT = 32;

/* The allow-list IS the security model, same as the repo one above. Only
 * firmware-* release tags, and only the two assets a snapshot publishes. */
const TAG_RE = /^firmware-[A-Za-z0-9._-]{1,64}$/;
const NAMES = {
    'fleet-index.json': 'application/json',
    'fleet-reports.tar.gz': 'application/gzip',
};

function allowed(tag, name) {
    if (!TAG_RE.test(tag) || !(name in NAMES)) return false;
    return !tag.includes('..'); /* no path games */
}

/* Fresh for five minutes, then a full day during which a stale copy is served
 * instantly while it revalidates behind the reader's back. Five minutes rather
 * than longer because a snapshot is re-uploaded to the same tag with --clobber
 * on every rerun; the long stale window is safe because revalidation is an
 * If-None-Match away, and a rerun changes the ETag. */
const CACHE_CONTROL = 'public, max-age=300, stale-while-revalidate=86400';
/* The remembered-tags entry is not a cached answer, it is a note to self, so
 * it is kept far longer than any response. Evicting it costs a rediscovery,
 * never a wrong answer: every remembered tag is re-probed before it is
 * offered. */
const REMEMBER_CONTROL = 'public, max-age=31536000';

/* An If-None-Match may list several, and may weaken them with W/. */
function matches(header, etag) {
    if (!header || !etag) return false;
    const want = etag.replace(/^W\//, '');
    return header.split(',').some((t) => t.trim().replace(/^W\//, '') === want);
}

function notModified(etag, origin) {
    const h = new Headers(cors(origin));
    if (etag) h.set('ETag', etag);
    h.set('Cache-Control', CACHE_CONTROL);
    return new Response(null, { status: 304, headers: h });
}

function cors(origin) {
    return {
        'Access-Control-Allow-Origin': origin,
        'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS',
        'Access-Control-Max-Age': '86400',
    };
}

function assetUrl(repo, tag, name) {
    return `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`;
}

/**
 * Tags that might carry a snapshot, from sources that do not share a quota.
 *
 * Deliberately not `/releases`: that endpoint embeds every release's full
 * asset list, ~18 MB of JSON for a firmware repo to learn a handful of tag
 * names.
 *
 * `api.github.com` allows 60 requests an hour PER IP, and a Worker's outbound
 * address is shared with every other customer on that edge, so the API call
 * is refused often. It used to be the only source of candidates, and when it
 * failed the list collapsed to the single tag the redirect gives -- an answer
 * indistinguishable from a real one, cached like a real one, which is how a
 * published release quietly stopped being offered.
 *
 * The tags feed costs no quota at all and is ~10 KB. The API still runs
 * because it reaches further back, but now it is an enrichment: losing it
 * costs a few older candidates rather than all of them.
 */
async function candidateTags(repo) {
    const seen = new Set();

    try {
        const res = await fetch(`https://github.com/${repo}/tags.atom`, {
            headers: { 'User-Agent': 'buildscope-fleet' },
        });
        if (res.ok) {
            const xml = await res.text();
            /* The href of each entry, not any tag URL quoted in its text: a
             * release note that links another project's tag is not ours. */
            for (const entry of xml.match(/<entry>[\s\S]*?<\/entry>/g) || []) {
                const m = /<link[^>]*href="[^"]*\/releases\/tag\/([^"]+)"/.exec(entry);
                if (m) seen.add(decodeURIComponent(m[1]));
            }
        }
    } catch { /* the API and the redirect are still to come */ }

    try {
        const res = await fetch(`https://api.github.com/repos/${repo}/tags?per_page=30`, {
            headers: { 'User-Agent': 'buildscope-fleet' },
        });
        if (res.ok) for (const t of await res.json()) if (t && t.name) seen.add(t.name);
    } catch { /* rate limited, most likely; the feed already answered */ }

    /* Newest release, from a redirect: no quota, nothing to parse. */
    try {
        const res = await fetch(`https://github.com/${repo}/releases/latest`, { redirect: 'manual' });
        const newest = (res.headers.get('Location') || '').split('/releases/tag/')[1] || '';
        if (newest) seen.add(newest);
    } catch { /* whatever the other two found stands */ }

    return [...seen].filter((t) => TAG_RE.test(t));
}

/**
 * Which releases carry a snapshot, remembering the ones already confirmed.
 *
 * A release that has a snapshot does not stop having one, so a tag confirmed
 * before is checked again rather than having to be rediscovered: enumeration
 * failing cannot take an existing release off the list, and a release stays
 * offered after it has aged out of the feed. Still probed every time, so one
 * whose assets are deleted does drop away.
 */
async function snapshotTags(repo, remembered = []) {
    const found = await candidateTags(repo);
    /* Date-stamped names, so lexical order is chronological. */
    const all = [...new Set([...found, ...remembered])].sort().reverse().slice(0, PROBE_LIMIT);

    const checked = await Promise.all(all.map(async (tag) => {
        try {
            const r = await fetch(assetUrl(repo, tag, 'fleet-index.json'), { method: 'HEAD', redirect: 'follow' });
            return r.ok ? tag : null;
        } catch {
            return null;
        }
    }));
    return checked.filter(Boolean);
}

export default {
    async fetch(request, env, ctx) {
        const origin = env.ALLOW_ORIGIN || '*';
        const url = new URL(request.url);

        if (request.method === 'OPTIONS')
            return new Response(null, { status: 204, headers: cors(origin) });
        if (request.method !== 'GET' && request.method !== 'HEAD')
            return new Response('method not allowed\n', { status: 405, headers: cors(origin) });
        if (url.pathname !== '/fleet' && url.pathname !== '/fleet/releases')
            return new Response('not found\n', { status: 404, headers: cors(origin) });

        const tag = url.searchParams.get('tag') || '';
        const name = url.searchParams.get('name') || '';
        const which = url.searchParams.get('repo') || DEFAULT_REPO;
        const repo = REPOS[which];
        if (!repo)
            return new Response('bad repo\n', { status: 400, headers: cors(origin) });

        /* Which releases have a snapshot. Small enough to hold in memory and
         * cache whole, unlike the assets below, which stream. */
        if (url.pathname === '/fleet/releases') {
            const listKey = new Request(`${url.origin}/fleet/releases?repo=${which}`, { method: 'GET' });
            const cache = caches.default;
            const hit = await cache.match(listKey);
            if (hit) {
                const h = new Headers(hit.headers);
                for (const [k, v] of Object.entries(cors(origin))) h.set(k, v);
                h.set('X-Fleet-Cache', 'HIT');
                return new Response(hit.body, { status: hit.status, headers: h });
            }
            /* Tags confirmed on an earlier pass, so a failed enumeration
             * cannot drop a release that is still published. */
            const knownKey = new Request(`${url.origin}/fleet/known?repo=${which}`, { method: 'GET' });
            let remembered = [];
            try {
                const prev = await cache.match(knownKey);
                if (prev) {
                    const body = await prev.json();
                    if (Array.isArray(body.tags)) remembered = body.tags.filter((t) => TAG_RE.test(t));
                }
            } catch { /* a note to self that cannot be read is simply absent */ }

            const tags = await snapshotTags(repo, remembered);
            if (request.method === 'GET' && tags.length) {
                ctx.waitUntil(cache.put(knownKey, new Response(JSON.stringify({ tags }), {
                    headers: { 'Content-Type': 'application/json', 'Cache-Control': REMEMBER_CONTROL },
                })));
            }
            const h = new Headers(cors(origin));
            h.set('Content-Type', 'application/json');
            h.set('Cache-Control', CACHE_CONTROL);
            const res = new Response(JSON.stringify({ tags }), { status: 200, headers: h });
            if (request.method === 'GET') ctx.waitUntil(cache.put(listKey, res.clone()));
            res.headers.set('X-Fleet-Cache', 'MISS');
            return res;
        }

        if (!allowed(tag, name))
            return new Response('bad tag/name\n', { status: 400, headers: cors(origin) });

        /* Key the cache on our own canonical URL. Caching the upstream URL would
         * never hit: GitHub 302s to a signed blob URL whose query string differs
         * on every request.
         *
         * The repo is part of the key: both repos carry the same tag and asset
         * names, and without it one would be served the other's bytes. */
        const key = new Request(`${url.origin}/fleet?repo=${which}&tag=${tag}&name=${name}`,
                                { method: 'GET' });
        const cache = caches.default;
        const inm = request.headers.get('If-None-Match');
        const hit = await cache.match(key);
        if (hit) {
            const etag = hit.headers.get('ETag');
            /* Nothing to send: the reader's copy is the one we have. */
            if (matches(inm, etag)) return notModified(etag, origin);
            const h = new Headers(hit.headers);
            for (const [k, v] of Object.entries(cors(origin))) h.set(k, v);
            h.set('X-Fleet-Cache', 'HIT');
            return new Response(hit.body, { status: hit.status, headers: h });
        }

        const upstream = await fetch(
            `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`,
            { method: request.method, redirect: 'follow' });

        if (!upstream.ok)
            return new Response(`upstream ${upstream.status}\n`,
                                { status: upstream.status === 404 ? 404 : 502, headers: cors(origin) });

        const h = new Headers(cors(origin));
        h.set('Content-Type', NAMES[name]);
        /* Pass the length through, or the viewer's progress bar goes indeterminate. */
        const len = upstream.headers.get('Content-Length');
        if (len) h.set('Content-Length', len);
        h.set('Cache-Control', CACHE_CONTROL);
        /* Upstream's validators, forwarded. Without them a reader whose copy has
         * expired can only re-download the whole asset, and no cache lifetime
         * would be safe, because nothing could notice a rerun replacing it. */
        for (const v of ['ETag', 'Last-Modified']) {
            const got = upstream.headers.get(v);
            if (got) h.set(v, got);
        }

        const res = new Response(upstream.body, { status: 200, headers: h });
        if (matches(inm, h.get('ETag'))) {
            /* The reader already has this version, so nothing goes back. The
             * response itself feeds the cache -- no clone, because tee-ing a
             * stream whose other half is never read would strand it. */
            if (request.method === 'GET') ctx.waitUntil(cache.put(key, res));
            return notModified(h.get('ETag'), origin);
        }
        /* clone() tees the stream: the client and the cache each get a copy, and
         * the bytes are still never held in memory. cache.put only takes GETs. */
        if (request.method === 'GET') ctx.waitUntil(cache.put(key, res.clone()));
        res.headers.set('X-Fleet-Cache', 'MISS'); /* after clone: cached copy stays clean */
        return res;
    },
};
