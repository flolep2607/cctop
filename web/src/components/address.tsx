import { useState, type FormEvent, type MouseEvent } from "react";
import { Globe, Pencil } from "lucide-react";
import { toast } from "sonner";
import { ask } from "@/lib/api";
import { withToken } from "@/lib/config";
import { addressPath, useAddress, type Address, type Done } from "@/lib/address";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { InputGroup, InputGroupAddon, InputGroupInput, InputGroupText } from "@/components/ui/input-group";

const labelOf = (host: string, zone: string | null) => (zone && host.endsWith("." + zone) ? host.slice(0, -zone.length - 1) : host);

/**
 * The rename itself. What Enter does is said before it is pressed — for the
 * dashboard, that its old links stop working and where the page goes — and a
 * refusal is the server's own sentence, shown under the field. Mounted
 * afresh each time it opens (its callers key it), so a reopened dialog starts
 * from the current name rather than whatever was typed and abandoned.
 */
export function AddressDialog({
  open,
  onOpenChange,
  address,
  session,
  agent,
  onRenamed,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  address: Address;
  /** The agent's session id; the dashboard when absent. */
  session?: string;
  agent?: string;
  onRenamed: (done: Done) => void;
}) {
  const now = address.host ?? "";
  const [name, setName] = useState(labelOf(now, address.zone));
  const [problem, setProblem] = useState("");
  const [busy, setBusy] = useState(false);

  const typed = name.trim().toLowerCase().replace(/\.$/, "");
  const target = typed ? (address.zone && !typed.endsWith("." + address.zone) ? typed + "." + address.zone : typed) : "";
  const dashboard = !session;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setProblem("");
    try {
      const response = await ask(addressPath(session), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name }),
      });
      onRenamed((await response.json()) as Done);
      onOpenChange(false);
    } catch (err) {
      const said = String((err as Error).message || err);
      setProblem(said);
      toast.error(said);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !busy && onOpenChange(o)}>
      <DialogContent className="sm:max-w-md">
        <form onSubmit={submit} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{dashboard ? "Move the dashboard" : "Choose this agent's address"}</DialogTitle>
            <DialogDescription>
              {dashboard ? (
                <>
                  Now at <span className="text-foreground font-mono">{now}</span>.
                </>
              ) : (
                <>
                  Where {agent ? <span className="text-foreground">{agent}</span> : "this agent"} goes when shared with <kbd>W</kbd> — now{" "}
                  <span className="text-foreground font-mono">{now}</span>.
                </>
              )}
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-1.5">
            <label className="text-sm font-medium" htmlFor="address-name">
              Address
            </label>
            <InputGroup>
              <InputGroupInput
                id="address-name"
                value={name}
                onChange={(e) => {
                  setName(e.target.value);
                  setProblem("");
                }}
                spellCheck={false}
                autoComplete="off"
                autoCapitalize="off"
                aria-invalid={!!problem}
                disabled={busy}
                autoFocus
              />
              {address.zone && (
                <InputGroupAddon align="inline-end">
                  <InputGroupText className="font-mono">.{address.zone}</InputGroupText>
                </InputGroupAddon>
              )}
            </InputGroup>
            {problem ? (
              <p className="text-destructive text-xs">{problem}</p>
            ) : (
              <p className="text-muted-foreground text-xs">
                Letters, digits and -.{!dashboard && address.default && <> Empty sends it back to <span className="font-mono">{address.default}</span>.</>}
              </p>
            )}
          </div>
          <p className="text-warning text-xs leading-relaxed">
            {dashboard ? (
              <>
                Links to <span className="font-mono">{now}</span> stop working; the page moves to{" "}
                <span className="font-mono">{target && target !== now ? target : "the new address"}</span>. Its token stays the same.
              </>
            ) : (
              <>Links on the old address stop working. Press W in the terminal for a link on the new one.</>
            )}
          </p>
          <DialogFooter>
            <Button type="button" variant="outline" disabled={busy} onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy}>
              {busy ? "Asking Cloudflare…" : dashboard ? "Move" : "Rename"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Right-click, like the terminal's: open the dialog instead of the browser's menu. */
const onContext = (open: () => void) => (e: MouseEvent) => {
  e.preventDefault();
  open();
};

/**
 * The dashboard's address in the header, while it can be chosen: the page on
 * the account's own domain, the full link, a serve that acts. After a move the
 * page goes to the new origin with its token — the old one no longer answers.
 */
export function DashboardAddress() {
  const [address] = useAddress();
  const [open, setOpen] = useState(false);
  if (!address?.renamable || !address.host) return null;
  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        // Not on a phone: the header has no room left there — the theme
        // switch is the next thing it would push off — and moving the
        // dashboard is a rare act best done where the address can be read.
        className="text-muted-foreground gap-1.5 font-mono text-xs font-normal max-sm:hidden"
        title="This page's address — click or right-click to choose another"
        onClick={() => setOpen(true)}
        onContextMenu={onContext(() => setOpen(true))}
      >
        <Globe />
        <span className="max-w-48 truncate max-md:hidden">{address.host}</span>
      </Button>
      <AddressDialog
        key={String(open)}
        open={open}
        onOpenChange={setOpen}
        address={address}
        onRenamed={(done) => {
          if (done.changed && done.origin) location.assign(withToken(done.origin + location.pathname + location.hash));
          else toast.success("The page stays on " + (done.host ?? address.host));
        }}
      />
    </>
  );
}

/**
 * The address `W` shares this agent on, with its own rename. Shown only where
 * shares go out on the account's domain; the button only where cctop can
 * write that domain's DNS.
 */
export function AgentAddress({ session, agent }: { session: string; agent: string }) {
  const [address, setAddress] = useAddress(session);
  const [open, setOpen] = useState(false);
  if (!address?.host) return null;
  const rename = address.renamable ? () => setOpen(true) : undefined;
  return (
    <span className="inline-flex items-center gap-1" title={address.why ?? "Where W shares this agent — right-click to choose another"}>
      <span className="opacity-70">Address</span>
      <span className="font-mono" onContextMenu={rename && onContext(rename)}>
        {address.host}
      </span>
      {rename && (
        <>
          <Button variant="ghost" size="icon-xs" className="size-5" aria-label="Choose this agent's address" onClick={rename}>
            <Pencil className="size-3" />
          </Button>
          <AddressDialog
            key={String(open)}
            open={open}
            onOpenChange={setOpen}
            address={address}
            session={session}
            agent={agent}
            onRenamed={(done) => {
              if (done.host) setAddress({ ...address, host: done.host });
              toast.success(
                done.changed
                  ? `${agent} is on ${done.host}${done.old ? ` — ${done.old} no longer answers` : ""}${done.note ? `. ${done.note}` : ""}`
                  : `${agent} stays on ${done.host}`,
              );
            }}
          />
        </>
      )}
    </span>
  );
}
