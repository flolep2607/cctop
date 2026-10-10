import { useEffect, useState } from "react";
import { getJson } from "@/lib/api";

// The machines whose rows this page shows that could not be read just now,
// and why: `--host` ssh targets and siblings on the Cloudflare account
// (`/api/hosts`). One poll for the whole page however many rows ask, on the
// pollers' own 15 s cadence, so a machine going offline turns its dot within
// one poll of the server noticing.

type Failed = Map<string, string>;

const POLL_MS = 15_000;
let current: Failed = new Map();
const listeners = new Set<(f: Failed) => void>();
let timer: ReturnType<typeof setInterval> | null = null;

function poll() {
  getJson<[string, string][]>("/api/hosts").then(
    (list) => {
      current = new Map(Array.isArray(list) ? list : []);
      for (const tell of listeners) tell(current);
    },
    () => {},
  );
}

/** Which machines are offline right now, by name, with the reason. */
export function useHosts(): Failed {
  const [failed, setFailed] = useState(current);
  useEffect(() => {
    listeners.add(setFailed);
    if (!timer) {
      poll();
      timer = setInterval(poll, POLL_MS);
    }
    return () => {
      listeners.delete(setFailed);
      if (!listeners.size && timer) {
        clearInterval(timer);
        timer = null;
      }
    };
  }, []);
  return failed;
}
