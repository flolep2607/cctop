import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { ask, getJson } from "@/lib/api";
import { withToken } from "@/lib/config";
import type { Session } from "@/lib/types";

/**
 * Notifications for sessions that start wanting something: whether they are
 * on, the switch, and the watcher.
 *
 * Two ways to be told, tried in order. Web Push first: the browser subscribes,
 * cctop sends the push on the same crossing its bell rings for, and the
 * service worker at `/sw.js` shows it — with the page closed, the phone locked,
 * the tab long since put to sleep. Where that is not on offer (an insecure
 * origin, a browser without it, an iPhone outside "Add to Home Screen", a
 * read-only link), the page raises the notification itself, which only works
 * while it is open.
 *
 * A notification marks the transition into waiting or asking — not a session
 * already waiting when the page opened, which the table shows anyway. Off
 * until switched on, because the permission prompt has to come from a click.
 */
export function useWantingNotifications(sessions: Session[] | null): { notify: boolean; pushing: boolean; toggle: () => Promise<void> } {
  const [notify, setNotify] = useState(false);
  // Whether this browser holds a push subscription. While it does, the server
  // is the one telling it, and the page stays quiet so nothing arrives twice.
  const [pushing, setPushing] = useState(false);
  const prev = useRef<Map<string, string> | null>(null);

  // A subscription made on an earlier visit is still live: say so, rather than
  // showing the switch off while the phone goes on being notified.
  useEffect(() => {
    void existing().then((sub) => {
      if (sub && Notification.permission === "granted") {
        setPushing(true);
        setNotify(true);
      }
    });
  }, []);

  useEffect(() => {
    if (!sessions) return;
    if (prev.current && notify && !pushing) {
      for (const s of sessions) {
        const was = prev.current.get(s.session_id);
        const body = (s.project || s.session_id.slice(0, 8)) + (s.title ? " · " + s.title : "");
        if (was && was !== "asking" && s.state === "asking") new Notification("Needs permission", { body, tag: s.session_id });
        if (was && was !== "waiting" && s.state === "waiting") new Notification("Waiting on you", { body, tag: s.session_id });
      }
    }
    prev.current = new Map(sessions.map((s) => [s.session_id, s.state]));
  }, [sessions, notify, pushing]);

  const toggle = async () => {
    if (notify) {
      setNotify(false);
      if (pushing) {
        setPushing(false);
        await unsubscribe().catch(() => toast.error("Could not stop push notifications on this device"));
      }
      return;
    }
    if (!("Notification" in window)) return void toast.error("This browser has no notifications");
    // Inside the click: a permission prompt no gesture asked for is refused.
    const granted = await Notification.requestPermission();
    if (granted !== "granted") return void toast.error("The browser refused notification permission for this page");
    setNotify(true);
    try {
      await subscribe();
      setPushing(true);
      toast.success("This device will be notified even with the page closed");
    } catch (e) {
      // The page's own notifications still work while it is open.
      toast.message("Notifications while this page is open", {
        description: e instanceof Error ? e.message : String(e),
      });
    }
  };
  return { notify, pushing, toggle };
}

function supported(): boolean {
  return window.isSecureContext && "serviceWorker" in navigator && "PushManager" in window;
}

async function existing(): Promise<PushSubscription | null> {
  if (!supported()) return null;
  try {
    const registration = await navigator.serviceWorker.getRegistration("/");
    return (await registration?.pushManager.getSubscription()) ?? null;
  } catch {
    return null;
  }
}

async function subscribe(): Promise<void> {
  if (!supported())
    throw new Error(
      window.isSecureContext
        ? "This browser cannot be pushed to — on an iPhone, add the page to the home screen first."
        : "Push needs https or localhost.",
    );
  // With the token on it: the worker reads it back to open a session's page.
  await navigator.serviceWorker.register(withToken("/sw.js"), { scope: "/" });
  const registration = await navigator.serviceWorker.ready;
  const { key } = await getJson<{ key: string }>("/api/push/key");
  const subscription =
    (await registration.pushManager.getSubscription()) ??
    (await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: fromBase64Url(key) }));
  await ask("/api/push/subscribe", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(subscription.toJSON()),
  });
}

async function unsubscribe(): Promise<void> {
  const subscription = await existing();
  if (!subscription) return;
  const endpoint = subscription.endpoint;
  await subscription.unsubscribe();
  await ask("/api/push/unsubscribe", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ endpoint }),
  });
}

/** The server's key as bytes: not every browser takes the base64url string. */
function fromBase64Url(text: string): Uint8Array<ArrayBuffer> {
  const b64 = text.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(text.length / 4) * 4, "=");
  const raw = atob(b64);
  const out = new Uint8Array(new ArrayBuffer(raw.length));
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}
