import { useEffect, useState } from "react";
import { ask, getJson } from "@/lib/api";
import { CAN_ACT } from "@/lib/config";

export type Level = "read" | "full";

/** Cloudflare Access on the connected account, as `/api/access` reports it. */
export type AccessState = {
  on: boolean;
  owner: string | null;
  invites: { who: string; level: Level }[];
  /** Whether a token link still opens the page through the tunnel. */
  public_links: boolean;
  /** Where token links go while Access is on. */
  link_host: string | null;
  page_host: string | null;
  editable: boolean;
  why: string | null;
};

export type Edit =
  | { op: "on"; owner: string }
  | { op: "off" }
  | { op: "invite"; who: string; level: Level }
  | { op: "remove"; who: string }
  | { op: "links"; on: boolean };

export type Done = { said: string; left: string[]; state: AccessState };

/**
 * The list, read once. Only the full link or the owner's login gets an
 * answer; anyone else gets 403, and then there is nothing to show.
 */
export function useAccess(): [AccessState | null, (s: AccessState) => void] {
  const [state, setState] = useState<AccessState | null>(null);
  useEffect(() => {
    if (!CAN_ACT) return;
    let live = true;
    getJson<AccessState>("/api/access").then(
      (s) => live && setState(s),
      () => {},
    );
    return () => {
      live = false;
    };
  }, []);
  return [state, setState];
}

/** One edit; a refusal throws the server's sentence. */
export async function edit(change: Edit): Promise<Done> {
  const response = await ask("/api/access", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(change),
  });
  return (await response.json()) as Done;
}
