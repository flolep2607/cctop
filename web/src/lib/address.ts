import { useEffect, useState } from "react";
import { getJson } from "@/lib/api";
import { CAN_ACT } from "@/lib/config";

/**
 * An address on the connected Cloudflare account's domain, as `/api/address`
 * reports it: the dashboard's, or the one `W` shares an agent on.
 */
export type Address = {
  host: string | null;
  zone: string | null;
  /** For an agent: where an empty name sends it back to. */
  default: string | null;
  renamable: boolean;
  why: string | null;
};

export type Done = { host: string | null; origin: string | null; old: string | null; changed: boolean; note: string | null };

export const addressPath = (session?: string) => "/api/address" + (session ? "/" + encodeURIComponent(session) : "");

/**
 * The address, read once. Only the full link asks: the read-only page never
 * shows these controls, and the server answers it 403.
 */
export function useAddress(session?: string): [Address | null, (a: Address) => void] {
  const [address, setAddress] = useState<Address | null>(null);
  useEffect(() => {
    if (!CAN_ACT) return;
    let live = true;
    getJson<Address>(addressPath(session)).then(
      (a) => live && setAddress(a),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [session]);
  return [address, setAddress];
}
