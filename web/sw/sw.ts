/// <reference lib="webworker" />
// The page's service worker: it shows a Web Push notification when the page is
// closed, and opens the session when the notification is tapped.
//
// Built on its own (vite.sw.config.ts) and served by cctop at `/sw.js`, because
// a service worker can only be registered from a URL — the app page is one file
// with everything inlined, and this is the one script that cannot be.
//
// The page registers it as `/sw.js?t=<token>`, so the token it needs to open a
// session page is read back off its own address rather than stored anywhere.

declare const self: ServiceWorkerGlobalScope;

interface Payload {
  title?: string;
  body?: string;
  session?: string;
}

const TOKEN = new URL(self.location.href).searchParams.get("t") || "";

function sessionUrl(id: string | undefined): string {
  const path = id ? "/session/" + encodeURIComponent(id) : "/";
  return TOKEN ? path + "?t=" + encodeURIComponent(TOKEN) : path;
}

// A new version takes over at once: there is no cache here for an old page to
// depend on, so nothing is lost by not waiting for every tab to close.
self.addEventListener("install", () => void self.skipWaiting());
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));

self.addEventListener("push", (event) => {
  let payload: Payload = {};
  try {
    payload = (event.data?.json() as Payload) || {};
  } catch {
    /* an unreadable push still says something needs looking at */
  }
  event.waitUntil(
    self.registration.showNotification(payload.title || "cctop", {
      body: payload.body || "",
      // One per session, so a session that rings twice replaces its own
      // notification rather than stacking a second.
      tag: payload.session || "cctop",
      data: { session: payload.session },
      icon: "/favicon.svg",
    }),
  );
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const url = sessionUrl((event.notification.data as Payload | undefined)?.session);
  event.waitUntil(
    (async () => {
      // An open cctop tab is reused rather than a second one opened beside it.
      const open = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
      for (const client of open) {
        if (new URL(client.url).origin !== self.location.origin) continue;
        await client.focus();
        if ("navigate" in client) {
          try {
            await client.navigate(url);
            return;
          } catch {
            /* an uncontrolled tab cannot be navigated; open one instead */
          }
        }
      }
      await self.clients.openWindow(url);
    })(),
  );
});
