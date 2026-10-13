import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import type { Session } from "@/lib/types";

/**
 * Desktop notifications for sessions that start wanting something: whether
 * they are on, the switch, and the watcher.
 *
 * A notification marks the transition into waiting or asking — not a session
 * already waiting when the page opened, which the table shows anyway. Off
 * until switched on, because the permission prompt has to come from a click.
 */
export function useWantingNotifications(sessions: Session[] | null): { notify: boolean; toggle: () => Promise<void> } {
  const [notify, setNotify] = useState(false);
  const prev = useRef<Map<string, string> | null>(null);
  useEffect(() => {
    if (!sessions) return;
    if (prev.current && notify) {
      for (const s of sessions) {
        const was = prev.current.get(s.session_id);
        const body = (s.project || s.session_id.slice(0, 8)) + (s.title ? " · " + s.title : "");
        if (was && was !== "asking" && s.state === "asking") new Notification("Needs permission", { body, tag: s.session_id });
        if (was && was !== "waiting" && s.state === "waiting") new Notification("Waiting on you", { body, tag: s.session_id });
      }
    }
    prev.current = new Map(sessions.map((s) => [s.session_id, s.state]));
  }, [sessions, notify]);

  const toggle = async () => {
    if (notify) return setNotify(false);
    if (!("Notification" in window)) return void toast.error("This browser has no notifications");
    // Inside the click: a permission prompt no gesture asked for is refused.
    const granted = await Notification.requestPermission();
    setNotify(granted === "granted");
    if (granted !== "granted") toast.error("The browser refused notification permission for this page");
  };
  return { notify, toggle };
}
