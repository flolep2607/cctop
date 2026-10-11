// The release mirror cctop's updater falls back on when GitHub's API refuses it.
//
// GitHub allows sixty unauthenticated API calls an hour per address, and an
// updater shares its address with whatever else is behind the same NAT. This
// never asks GitHub anything: the release workflow pushes the latest release
// here when one is published or its notes are edited, and readers get that copy
// from KV, behind Cloudflare's cache. The JSON is GitHub's own, so cctop parses
// it with the same code either way: the latest release, to update by, and the
// recent list cut to tag and notes, to show what changed in between.

async function authorised(request, token) {
  if (!token) return false;
  const given = new TextEncoder().encode(request.headers.get("authorization") ?? "");
  const wanted = new TextEncoder().encode(`Bearer ${token}`);
  return given.byteLength === wanted.byteLength && crypto.subtle.timingSafeEqual(given, wanted);
}

// Each path, the KV key it is kept under, and what a push to it must look like.
const ROUTES = {
  "/releases/latest": {
    key: "latest",
    valid: (r) => typeof r?.tag_name === "string" && Array.isArray(r.assets),
    expected: "GitHub's latest release",
  },
  "/releases": {
    key: "list",
    valid: (l) => Array.isArray(l) && l.every((r) => typeof r?.tag_name === "string"),
    expected: "a list of releases",
  },
};

export default {
  async fetch(request, env) {
    const route = ROUTES[new URL(request.url).pathname];
    if (!route) {
      return new Response("not found\n", { status: 404 });
    }

    if (request.method === "PUT") {
      if (!(await authorised(request, env.PUSH_TOKEN))) {
        return new Response("unauthorised\n", { status: 401 });
      }
      const pushed = await request.json().catch(() => null);
      if (!route.valid(pushed)) {
        return new Response(`expected ${route.expected}\n`, { status: 400 });
      }
      await env.RELEASES.put(route.key, JSON.stringify(pushed));
      return new Response(`stored ${route.key}\n`);
    }

    if (request.method === "GET") {
      const stored = await env.RELEASES.get(route.key);
      return stored
        ? new Response(stored, {
            headers: {
              "content-type": "application/json",
              "cache-control": "public, max-age=300",
            },
          })
        : new Response("no release yet\n", { status: 404 });
    }

    return new Response("method not allowed\n", { status: 405 });
  },
};
