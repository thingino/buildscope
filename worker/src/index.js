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

function cors(origin) {
    return {
        'Access-Control-Allow-Origin': origin,
        'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS',
        'Access-Control-Max-Age': '86400',
    };
}

export default {
    async fetch(request, env, ctx) {
        const origin = env.ALLOW_ORIGIN || '*';
        const url = new URL(request.url);

        if (request.method === 'OPTIONS')
            return new Response(null, { status: 204, headers: cors(origin) });
        if (request.method !== 'GET' && request.method !== 'HEAD')
            return new Response('method not allowed\n', { status: 405, headers: cors(origin) });
        if (url.pathname !== '/fleet')
            return new Response('not found\n', { status: 404, headers: cors(origin) });

        const tag = url.searchParams.get('tag') || '';
        const name = url.searchParams.get('name') || '';
        const which = url.searchParams.get('repo') || DEFAULT_REPO;
        const repo = REPOS[which];
        if (!repo)
            return new Response('bad repo\n', { status: 400, headers: cors(origin) });
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
        const hit = await cache.match(key);
        if (hit) {
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
        /* Minutes, not a day. Unlike a released image, a snapshot is re-uploaded
         * to the same tag with --clobber on every rerun, so caching it hard
         * would serve yesterday's fleet from a tag that has since changed. */
        h.set('Cache-Control', 'public, max-age=300');

        const res = new Response(upstream.body, { status: 200, headers: h });
        /* clone() tees the stream: the client and the cache each get a copy, and
         * the bytes are still never held in memory. cache.put only takes GETs. */
        if (request.method === 'GET') ctx.waitUntil(cache.put(key, res.clone()));
        res.headers.set('X-Fleet-Cache', 'MISS'); /* after clone: cached copy stays clean */
        return res;
    },
};
