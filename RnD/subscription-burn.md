# Subscription burn — what you paid for and did not use

**Status:** design note, nothing built.
**Origin:** the user's idea, 2026-09-02, while reading
[codeburn.md](codeburn.md). Not from codeburn — they do not attempt this.

---

## The idea

A subscription window is use-it-or-lose-it. Claude gives you a five-hour
allowance and a seven-day allowance; Codex gives you a five-hour one. When a
window resets, whatever you did not spend is gone. Nobody bills you for it and
nobody tells you about it, so the most expensive thing about a $200 plan is
invisible: the weeks you used a third of it.

Every tool in this space, cctop included, shows the same number — *how full is
the window right now*. That answers "can I keep working". It does not answer
"am I paying for a plan I do not use", which is the question with money in it.

The other half is the exchange rate. cctop already estimates what a session's
tokens would have cost at retail API rates. Set that against what the
subscription costs and you get the thing a subscriber actually wants to know:
whether the plan is earning its price, and by how much.

## What already exists

More than expected. `src/quota.rs` fetches, per profile, per provider:

- **Claude** — `api.anthropic.com/api/oauth/usage`, giving `rate_limit_tier`
  (the plan name), and a `five_hour` and `seven_day` window each with a
  `utilization` percentage and a `resets_at`.
- **Codex** — a `primary_window` five-hour figure with `used_percent` and
  `reset_at`, plus a separate code-review limit. The secondary window is
  deliberately not shown, because it measures throttle pressure rather than
  consumption against a cap.

`src/ui/render.rs` already colours a window by **pace**: it compares
utilization against an even spend across the window, so 40% used at the halfway
mark is calm and 40% used in the first hour is not. That function
(`quota_color`) is already most of the reasoning this feature needs — it knows
the window length, the reset time, and therefore the sustainable rate.

`src/ui/spark.rs` has a sparkline and a line chart. `src/serve/report.rs`
builds a per-session report for the browser. `Plan` in `src/pricing.rs` knows
`max` and `included` and which providers a plan covers.

So the surfaces exist and the live number exists.

## What is missing: history

Utilization is a point sample. The API never says what the window *peaked* at,
only what it holds now, and the moment it resets the evidence is destroyed. To
say anything about a week that has ended, cctop has to have been writing the
samples down.

That is the whole of the new state: an append-only sample log, one row per
fetch, per provider, per profile, per window:

```
timestamp, provider, profile, window label, pct, resets_at, plan tier
```

Small — a few hundred bytes an hour at cctop's existing poll rate, and only
while cctop is running. Everything below is derived from it, which is the right
shape: no new source of truth, just a record of one that erases itself.

## What can be derived from that

A window reset is visible without being told: `resets_at` moves, or `pct` falls.
The sample immediately before is the **peak of record** for the window that just
ended.

- **Forfeited allowance.** `100 − peak` for each completed window. Per week,
  per five hours, and averaged over however many windows the log holds.
- **The evolution the user asked for.** Utilization against time, at whatever
  granularity the samples support — hours within a five-hour window, days within
  a seven-day one. `spark.rs` draws this today.
- **Pace, extended backwards.** `quota_color` already knows the sustainable
  rate. With history it can say not just "you are ahead of budget" but "you have
  been ahead of budget every day this week", which is a different sentence.
- **The exchange rate.** Sum the retail-API estimate for sessions in the window,
  against the plan's price. Two numbers side by side: *this plan costs $X, you
  consumed $Y of tokens at retail*. cctop already computes Y per session.
- **Money left on the table.** Forfeited percentage against the plan price. This
  is the headline and it is also the number most likely to be wrong — see below.

## Where this can lie, and what to do about it

cctop's register is to show the doubt rather than hide it — the Unaccounted bar
in the context panel is the precedent, and the cost page is written the same
way. Three things here need the same treatment.

**Sampling gaps are the big one.** cctop only samples while it is running. If it
was closed during the hours you did your heaviest work, the peak of record is
below the true peak, so usage is understated and forfeit is *overstated*.
Forfeit is therefore an **upper bound**, not a measurement, and it must be
labelled as one. A window whose coverage is thin should say so rather than
quietly report a confident number — record observed coverage (samples present
against samples possible) alongside the peak, and let the display degrade to
"at least 60% used" when coverage is poor.

**Percentages are not dollars.** A window at 50% has not necessarily consumed
half the plan's value; the provider's mapping from tokens to utilization is
undocumented and not obviously linear across models. Any dollar figure derived
from a percentage is an illustration. Saying "you forfeited $80" is a claim
cctop cannot support. Saying "you left 40% of the week's allowance unused, on a
$200 plan" is true and lands just as hard.

**Plan prices change and vary.** Codeburn hardcodes presets and dates them
("public prices, April 2026"). cctop should take the price from the user rather
than guess it, and show the ratio unpriced until it is told.

## Surfaces

The user named three, in increasing order of effort:

1. **The live TUI.** The Limits pane already exists and already has each
   window. It gains a sparkline of the window's history and, at the end of a
   window, the forfeited figure. This is the smallest useful version and it is
   worth shipping alone.
2. **`cctop serve`.** The same thing in the browser, live over the existing SSE
   stream. This is where the charts can be larger than a terminal cell, and
   where a phone is a genuinely good place to look at a weekly figure.
3. **A full HTML report.** A standalone file, self-contained, coverable by
   `--from`/`--to`, that answers "what did this subscription do for me this
   month". `src/serve/report.rs` already builds a per-session report; this is
   the account-level sibling. It is also the artefact somebody forwards to
   whoever pays for the plan, which is a use cctop does not currently serve at
   all.

## Open questions

- **How often to sample.** Too rare and the peak is missed; too often and it is
  an API call cctop makes on someone's behalf all day. The reset time is known,
  so sampling can be adaptive — dense near a reset boundary, sparse in the
  middle of a quiet window.
- **Where the log lives, and whether it is shared.** Per profile, certainly.
  Whether `--host` merges another machine's log is a real question: the quota
  is per *account*, not per machine, so two machines on one login are sampling
  the same window and their logs should union rather than sum.
- **How far back to keep.** A rolling year is a few megabytes and makes
  month-over-month possible. Anything shorter throws away the comparison that
  makes the number interesting.
- **What happens on a plan change.** The tier is in the sample, so a change is
  detectable; the history either side of it is not comparable and the display
  should break rather than average across it.

## What to build first

The sample log and the forfeited figure for the seven-day Claude window, shown
in the Limits pane, with the coverage caveat. That is the whole idea in its
smallest honest form. Everything else — Codex, the five-hour window, the charts,
the dollar ratio, the HTML report — is addition, and each piece is separately
useful, so none of it needs to be designed now.
