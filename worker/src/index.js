// The release mirror cctop's updater falls back on when GitHub's API refuses it.
//
// GitHub allows sixty unauthenticated API calls an hour per address, and an
// updater shares its address with whatever else is behind the same NAT. This
// never asks GitHub anything: the release workflow pushes the latest release
// here when one is published or its notes are edited, and readers get that copy
// from KV, behind Cloudflare's cache. The JSON is GitHub's own, so cctop parses
// it with the same code either way. Only the latest: that is all an update
// needs, and the notes of the versions in between are GitHub's to give.

async function authorised(request, token) {
  if (!token) return false;
  const given = new TextEncoder().encode(request.headers.get("authorization") ?? "");
  const wanted = new TextEncoder().encode(`Bearer ${token}`);
  return given.byteLength === wanted.byteLength && crypto.subtle.timingSafeEqual(given, wanted);
}

export default {
  async fetch(request, env) {
    if (new URL(request.url).pathname !== "/releases/latest") {
      return new Response("not found\n", { status: 404 });
    }

    if (request.method === "PUT") {
      if (!(await authorised(request, env.PUSH_TOKEN))) {
        return new Response("unauthorised\n", { status: 401 });
      }
      const release = await request.json().catch(() => null);
      if (typeof release?.tag_name !== "string" || !Array.isArray(release.assets)) {
        return new Response("expected GitHub's latest release\n", { status: 400 });
      }
      await env.RELEASES.put("latest", JSON.stringify(release));
      return new Response(`stored ${release.tag_name}\n`);
    }

    if (request.method === "GET") {
      const latest = await env.RELEASES.get("latest");
      return latest
        ? new Response(latest, {
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
