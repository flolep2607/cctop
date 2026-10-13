import { useHosts } from "@/hooks/use-hosts";

// Machines that could not be read — ssh hosts and siblings on the account —
// for as long as they cannot be.
export function HostBanners() {
  const failed = [...useHosts()];
  return (
    <>
      {failed.map(([host, why]) => (
        <div key={host} className="border-destructive/40 bg-destructive/5 text-destructive rounded-md border px-3 py-1.5 text-xs">
          {host} could not be read: {why}
        </div>
      ))}
    </>
  );
}
