// The release mirror cctop's updater falls back on when GitHub's API refuses it.
//
// GitHub allows sixty unauthenticated API calls an hour per address, and an
// updater shares its address with whatever else is behind the same NAT. This
// never asks GitHub anything: the release workflow pushes the latest release
// here when one is published or its notes are edited, and readers get that copy
// from KV, behind Cloudflare's cache. The JSON is GitHub's own, so cctop parses
// it with the same code either way: the latest release, to update by, and the
// recent list cut to tag and notes, to show what changed in between.
//
// The release files themselves are kept too, in R2 under /assets/<tag>/<name>,
// for when GitHub is down altogether rather than only rate-limiting. The
// updater derives that URL from the tag and the asset's name, so the release
// JSON above stays GitHub's own. Nothing served from here is trusted for
// coming from here: every archive is signed in the release workflow with a key
// this Worker never sees, and the updater installs a mirrored archive only
// when that signature checks out (`fetch_archive` in crates/core/src/update.rs).
// A leaked PUSH_TOKEN can make the mirror serve junk, not a binary anyone runs.

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

// cctop's anonymous daily usage report (crates/core/src/telemetry.rs), checked
// key by key: anything not named here is refused, so a field can only start
// being collected by a change to this file.
const FEATURES = ["tui", "serve", "workspace", "tunnel", "yolo", "update"];
const AGENTS = ["claude", "codex", "cursor", "devin", "gemini", "opencode", "pi", "windsurf"];
const TELEMETRY_MAX = 4096;

const short = (s, max) => typeof s === "string" && s.length > 0 && s.length <= max;
const keysWithin = (o, allowed) =>
  o !== null && typeof o === "object" && !Array.isArray(o) && Object.keys(o).every((k) => allowed.includes(k));
const counts = (o, allowed) =>
  keysWithin(o, allowed) &&
  Object.values(o).every((n) => Number.isInteger(n) && n >= 0 && n <= 1_000_000);

function validReport(r) {
  return (
    keysWithin(r, ["id", "version", "os", "arch", "features", "agents", "crash"]) &&
    typeof r.id === "string" &&
    /^[0-9a-f]{32}$/.test(r.id) &&
    [r.version, r.os, r.arch].every((s) => short(s, 32) && /^[\w.+-]+$/.test(s)) &&
    counts(r.features, FEATURES) &&
    counts(r.agents, AGENTS) &&
    (r.crash === undefined ||
      (keysWithin(r.crash, ["location", "message"]) &&
        short(r.crash.location, 200) &&
        typeof r.crash.message === "string" &&
        r.crash.message.length <= 200))
  );
}

async function telemetry(request, env) {
  if (request.method !== "POST") {
    return new Response("method not allowed\n", { status: 405 });
  }
  if (Number(request.headers.get("content-length") ?? 0) > TELEMETRY_MAX) {
    return new Response("too large\n", { status: 413 });
  }
  const text = await request.text();
  if (text.length > TELEMETRY_MAX) {
    return new Response("too large\n", { status: 413 });
  }
  let report;
  try {
    report = JSON.parse(text);
  } catch {
    report = null;
  }
  if (!validReport(report)) {
    return new Response("expected a cctop usage report\n", { status: 400 });
  }
  // One row per thing counted, so each is a plain SUM(double1) grouped by
  // blob5, with blob4 the row's kind. The index is the install's random id,
  // which is what COUNT(DISTINCT index1) and retention are read from.
  const base = [report.version, report.os, report.arch];
  const indexes = [report.id];
  const rows = [
    ...Object.entries(report.features).map(([name, n]) => ["feature", name, n]),
    ...Object.entries(report.agents).map(([name, n]) => ["agents", name, n]),
  ];
  for (const [kind, name, n] of rows) {
    env.TELEMETRY.writeDataPoint({ indexes, blobs: [...base, kind, name], doubles: [n] });
  }
  env.TELEMETRY.writeDataPoint({ indexes, blobs: [...base, "report"], doubles: [1] });
  if (report.crash) {
    env.TELEMETRY.writeDataPoint({
      indexes,
      blobs: [...base, "crash", report.crash.location, report.crash.message],
      doubles: [1],
    });
  }
  return new Response(null, { status: 204 });
}

// A tag as the release workflow cuts them, and a release file's name: no
// slashes and no dot-dot, so a key can never step outside its tag's folder.
const TAG = /^v\d+\.\d+\.\d+[\w.+-]*$/;
const NAME = /^[\w+-][\w.+-]*$/;
// An archive is a few MB; this only stops a mistake from filling the bucket.
const ASSET_MAX = 64 << 20;
// How many releases' files are kept. The updater only ever fetches the
// latest, so the rest are slack for an updater that read the release a moment
// before the next one was pushed.
const KEEP_RELEASES = 3;

function contentType(name) {
  if (name.endsWith(".tar.gz")) return "application/gzip";
  if (name.endsWith(".sha256") || name.endsWith(".sig")) return "text/plain; charset=utf-8";
  return "application/octet-stream";
}

// Dotted numbers, newest first; a suffix sorts as nothing, as in the updater.
function byVersionDescending(a, b) {
  const parts = (t) => t.replace(/^v/, "").split(/[-+]/)[0].split(".").map((p) => Number(p) || 0);
  const [x, y] = [parts(a), parts(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    const d = (y[i] ?? 0) - (x[i] ?? 0);
    if (d !== 0) return d;
  }
  return 0;
}

// Delete every release's files but the newest KEEP_RELEASES, so the bucket
// stays a few tens of MB however many releases there have been.
async function prune(bucket) {
  const tags = [];
  let cursor;
  do {
    const page = await bucket.list({ delimiter: "/", cursor });
    tags.push(...page.delimitedPrefixes.map((p) => p.slice(0, -1)));
    cursor = page.truncated ? page.cursor : undefined;
  } while (cursor);
  for (const tag of tags.sort(byVersionDescending).slice(KEEP_RELEASES)) {
    let more;
    do {
      const page = await bucket.list({ prefix: `${tag}/` });
      if (page.objects.length) await bucket.delete(page.objects.map((o) => o.key));
      more = page.truncated;
    } while (more);
  }
}

async function asset(request, env, tag, name) {
  const key = `${tag}/${name}`;
  if (request.method === "PUT") {
    if (!(await authorised(request, env.PUSH_TOKEN))) {
      return new Response("unauthorised\n", { status: 401 });
    }
    // R2 takes a stream only when its length is known up front.
    const length = Number(request.headers.get("content-length"));
    if (!Number.isInteger(length) || length <= 0 || length > ASSET_MAX || !request.body) {
      return new Response("expected a release file with its content-length\n", { status: 411 });
    }
    await env.RELEASE_FILES.put(key, request.body, {
      httpMetadata: { contentType: contentType(name) },
    });
    await prune(env.RELEASE_FILES);
    return new Response(`stored ${key}\n`);
  }

  if (request.method === "GET" || request.method === "HEAD") {
    const object =
      request.method === "GET" ? await env.RELEASE_FILES.get(key) : await env.RELEASE_FILES.head(key);
    if (!object) {
      return new Response("no such release file\n", { status: 404 });
    }
    const headers = new Headers();
    object.writeHttpMetadata(headers);
    headers.set("content-length", String(object.size));
    headers.set("etag", object.httpEtag);
    // A tag's files never change once pushed, so any cache may keep them.
    headers.set("cache-control", "public, max-age=31536000, immutable");
    return new Response(request.method === "GET" ? object.body : null, { headers });
  }

  return new Response("method not allowed\n", { status: 405 });
}

export default {
  async fetch(request, env) {
    const path = new URL(request.url).pathname;
    if (path === "/telemetry") {
      return telemetry(request, env);
    }
    const file = path.match(/^\/assets\/([^/]+)\/([^/]+)$/);
    if (file) {
      const [, tag, name] = file;
      return TAG.test(tag) && NAME.test(name)
        ? asset(request, env, tag, name)
        : new Response("not found\n", { status: 404 });
    }
    const route = ROUTES[path];
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
