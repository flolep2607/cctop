# `cctop burn` — what the subscription was paid for and did not use

[← back to the README](../README.md)

A rate-limit window is use-it-or-lose-it. Claude gives an account five hours and
seven days; Codex gives it five. When one resets, whatever was not spent is gone
— nobody is billed for it and nobody mentions it, so the most expensive thing
about a subscription is invisible: the weeks that used a third of it.

The Limits pane already answers *how full is the window now*, which is what
decides whether there is room to keep working. This answers the other question:
whether the plan was worth buying.

```bash
cctop burn
cctop burn --json
```

```
  claude/default/7d  (max_20x)
  ▇▅▄▇▃▆  peak of each window, oldest first
  33% went unused on average, across 6 well-observed windows
```

When there is enough history, the same figure appears on the Limits pane beside
the live percentages — `~33% of 7d unused`.

## Why it takes a while to say anything

The provider reports only how full the window is *now*. It never says what a
window peaked at, and the moment it resets the evidence is gone. So cctop writes
the readings down as it sees them, and reconstructs each completed window from
the samples taken while it was open.

Two consequences:

- **A window has to reset before it can be reported.** A few hours for Codex, a
  few days for Claude. The window currently running is deliberately left out —
  calling its unused share "wasted" while there is still time to spend it would
  be wrong.
- **Nothing is claimed from a single window.** One quiet week is a quiet week.
  The figure appears once there are at least two completed windows to average.

The log lives beside the other caches, holds a rolling year, and thins its older
half rather than truncating when it grows — so old history keeps its shape at
half the resolution instead of ending at a cliff.

## The honest limit

**cctop only samples while it is running.** A window whose busiest hours
happened with cctop closed has a recorded peak below its real one, so usage is
understated and the unused share is *overstated*.

What this reports is therefore **an upper bound on what was wasted, not a
measurement of it**. Two things keep that from being misleading:

- Every window records how much of its own length was actually observed.
- A window observed for less than two thirds of its life is **left out of the
  average entirely**, and the report says how many were dropped and why. A
  barely-watched window would drag the figure toward "you used none of it",
  which is precisely the wrong direction for a number that might prompt someone
  to downgrade a plan.

If nothing is well-observed enough, the answer is that there is no figure —
never a zero.

## Percentages are not dollars

How a provider maps tokens onto a utilization percentage is undocumented and not
obviously linear across models. So cctop will say you left 40% of the week's
allowance unused, and will not turn that into "you wasted $80" — that would be
an illustration wearing the clothes of a measurement.

The plan name is recorded alongside each sample, because a plan change makes the
history either side of it incomparable.

## What is not built yet

The charts over hours and days, the browser view, and the standalone HTML
report. The sample log carries what they need; nothing reads it that way yet.
