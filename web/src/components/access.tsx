import { useState, type FormEvent } from "react";
import { Link2, Link2Off, ShieldCheck, ShieldOff, X } from "lucide-react";
import { toast } from "sonner";
import { edit, useAccess, type AccessState, type Edit, type Level } from "@/lib/access";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";

/** The two levels, as a pair of buttons. */
function LevelPicker({ value, onChange, disabled }: { value: Level; onChange: (l: Level) => void; disabled?: boolean }) {
  return (
    <ToggleGroup
      type="single"
      variant="outline"
      size="sm"
      spacing={0}
      value={value}
      disabled={disabled}
      onValueChange={(v) => v && onChange(v as Level)}
      aria-label="What they may do"
    >
      <ToggleGroupItem value="read" className="px-2 text-xs">
        Read-only
      </ToggleGroupItem>
      <ToggleGroupItem value="full" className="px-2 text-xs">
        Full
      </ToggleGroupItem>
    </ToggleGroup>
  );
}

/**
 * Who may log in, and how: the same edits as `cctop tunnel access` and the
 * terminal's Cloudflare popup. Every button is one call to Cloudflare; the
 * dialog shows what the server says the list is afterwards, never a guess.
 */
function AccessDialog({
  open,
  onOpenChange,
  state,
  onChanged,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  state: AccessState;
  onChanged: (s: AccessState) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState("");
  const [owner, setOwner] = useState("");
  const [who, setWho] = useState("");
  const [level, setLevel] = useState<Level>("read");
  const [confirmOff, setConfirmOff] = useState(false);
  const locked = busy || !state.editable;

  const run = async (change: Edit, after?: () => void) => {
    setBusy(true);
    setProblem("");
    try {
      const done = await edit(change);
      onChanged(done.state);
      toast.success(done.said);
      for (const item of done.left) toast.warning("Left on Cloudflare to delete by hand: " + item);
      after?.();
    } catch (err) {
      const said = String((err as Error).message || err);
      setProblem(said);
      toast.error(said);
    } finally {
      setBusy(false);
    }
  };

  const turnOn = (e: FormEvent) => {
    e.preventDefault();
    if (owner.trim()) run({ op: "on", owner: owner.trim() });
  };
  const invite = (e: FormEvent) => {
    e.preventDefault();
    if (who.trim()) run({ op: "invite", who: who.trim(), level }, () => setWho(""));
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !busy && onOpenChange(o)}>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Who may log in</DialogTitle>
          <DialogDescription>
            {state.on ? (
              <>
                Cloudflare Access asks for a login on <span className="text-foreground font-mono break-all">{state.page_host}</span>. A code is
                emailed to whoever asks; nobody needs a Cloudflare account. <span className="text-foreground">{state.owner}</span> owns it.
              </>
            ) : (
              <>
                The page opens with its token link, and anyone holding the link is in. Turned on, Cloudflare Access asks for a login instead: a
                code emailed to you, or to whoever you invite.
              </>
            )}
          </DialogDescription>
        </DialogHeader>

        {!state.editable && state.why && <p className="text-warning text-xs">{state.why}</p>}

        {!state.on ? (
          <form onSubmit={turnOn} className="grid gap-1.5">
            <label className="text-sm font-medium" htmlFor="access-owner">
              Your email — the owner, always full
            </label>
            <div className="flex gap-2">
              <Input
                id="access-owner"
                type="email"
                value={owner}
                onChange={(e) => setOwner(e.target.value)}
                placeholder="you@example.com"
                autoComplete="email"
                spellCheck={false}
                disabled={locked}
                aria-invalid={!!problem}
              />
              <Button type="submit" disabled={locked || !owner.trim()}>
                <ShieldCheck />
                {busy ? "Asking…" : "Turn on"}
              </Button>
            </div>
          </form>
        ) : (
          <>
            <div className="grid gap-2">
              <div className="text-sm font-medium">Invited</div>
              {state.invites.length === 0 ? (
                <p className="text-muted-foreground text-xs">No one else yet.</p>
              ) : (
                <ul className="divide-border grid divide-y rounded-lg border">
                  {state.invites.map((invite) => (
                    <li key={invite.who} className="flex flex-wrap items-center gap-2 px-2.5 py-1.5">
                      <span className="min-w-0 flex-1 truncate font-mono text-sm" title={invite.who}>
                        {invite.who.startsWith("@") ? (
                          <>
                            <span className="text-muted-foreground font-sans text-xs">everyone at </span>
                            {invite.who}
                          </>
                        ) : (
                          invite.who
                        )}
                      </span>
                      <LevelPicker
                        value={invite.level}
                        disabled={locked}
                        onChange={(l) => l !== invite.level && run({ op: "invite", who: invite.who, level: l })}
                      />
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={"Remove " + invite.who}
                        disabled={locked}
                        onClick={() => run({ op: "remove", who: invite.who })}
                      >
                        <X />
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
              <form onSubmit={invite} className="flex flex-wrap items-center gap-2">
                <Input
                  className="min-w-40 flex-1"
                  value={who}
                  onChange={(e) => {
                    setWho(e.target.value);
                    setProblem("");
                  }}
                  placeholder="name@company.com or @company.com"
                  aria-label="Email or @domain to invite"
                  spellCheck={false}
                  autoCapitalize="off"
                  disabled={locked}
                  aria-invalid={!!problem}
                />
                <LevelPicker value={level} onChange={setLevel} disabled={locked} />
                <Button type="submit" size="sm" disabled={locked || !who.trim()}>
                  Invite
                </Button>
              </form>
            </div>

            <Separator />

            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <div className="grid min-w-0 flex-1 gap-0.5">
                <div className="flex items-center gap-2 text-sm font-medium">
                  Public token links
                  <Badge variant="outline" className={state.public_links ? "text-success border-success/40" : "text-warning border-warning/40"}>
                    {state.public_links ? "on" : "off"}
                  </Badge>
                </div>
                <p className="text-muted-foreground text-xs leading-relaxed">
                  {state.public_links ? (
                    <>
                      For people who cannot log in, a token link works on
                      <span className="text-foreground block font-mono break-all">{state.link_host ?? state.page_host}</span>
                    </>
                  ) : (
                    <>From outside, only a login gets in. A token still works on this machine.</>
                  )}
                </p>
              </div>
              <Button variant="outline" size="sm" disabled={locked} onClick={() => run({ op: "links", on: !state.public_links })}>
                {state.public_links ? <Link2Off /> : <Link2 />}
                {state.public_links ? "Turn off" : "Turn on"}
              </Button>
            </div>

            <Separator />

            <div className="flex flex-wrap items-center justify-end gap-2">
              {confirmOff && (
                <span className="text-warning mr-auto text-xs">The token link opens the page again, and the invites are forgotten.</span>
              )}
              {confirmOff ? (
                <>
                  <Button variant="outline" size="sm" disabled={busy} onClick={() => setConfirmOff(false)}>
                    Keep
                  </Button>
                  <Button variant="destructive" size="sm" disabled={locked} onClick={() => run({ op: "off" }, () => setConfirmOff(false))}>
                    <ShieldOff />
                    Turn Access off
                  </Button>
                </>
              ) : (
                <Button variant="ghost" size="sm" className="text-muted-foreground" disabled={locked} onClick={() => setConfirmOff(true)}>
                  <ShieldOff />
                  Turn Access off…
                </Button>
              )}
            </div>
          </>
        )}

        {problem && <p className="text-destructive text-xs">{problem}</p>}
      </DialogContent>
    </Dialog>
  );
}

/**
 * The header's way to Access, for the full link or the owner's login only —
 * the server answers anyone else 403, and then nothing is drawn. Shown only
 * where there is an account to put it on.
 */
export function AccessButton() {
  const [state, setState] = useAccess();
  const [open, setOpen] = useState(false);
  if (!state || (!state.on && !state.editable)) return null;
  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        className="text-muted-foreground gap-1.5 text-xs font-normal"
        title={state.on ? "Cloudflare Access is on — who may log in" : "Put this page behind a login"}
        aria-label="Who may log in"
        onClick={() => setOpen(true)}
      >
        {state.on ? <ShieldCheck className="text-success" /> : <ShieldOff />}
        <span className="max-md:hidden">{state.on ? `Access · ${state.invites.length + 1}` : "Access off"}</span>
      </Button>
      <AccessDialog key={String(open)} open={open} onOpenChange={setOpen} state={state} onChanged={setState} />
    </>
  );
}
