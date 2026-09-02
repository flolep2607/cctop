# RnD

Working notes. Nothing in here ships, and nothing in here is a promise.

This is where research goes before it is a decision: what other tools do and how
they do it, ideas that have not earned an issue yet, half-built trials, and the
measurements that either justified a change or killed it. `docs/` is for people
using cctop and `CONTRIBUTING.md` is for people changing it — this folder is for
the step before either exists.

Two rules keep it useful rather than a graveyard:

- **Say where it came from and when.** A note about another project's design is
  only trustworthy against the date it was read; those projects move. Every file
  here carries its sources and the date they were fetched.
- **Say what happened.** A trial that failed is worth more written down than
  deleted — the next person to have the same idea should find out here that it
  was tried. Record the result, including "abandoned, and why".

## What is here

| | |
|---|---|
| [codeburn.md](codeburn.md) | Ideas read out of codeburn, an AI-spend tracker with overlapping scope, with the verdict on each |
| [optimize-and-compare.md](optimize-and-compare.md) | Design for `cctop optimize` and `cctop compare`, accepted out of the above |
| [subscription-burn.md](subscription-burn.md) | Design for showing how much of a subscription window is forfeited unused |
| [follow-ups.md](follow-ups.md) | Defects and gaps noticed while researching, with where each was found |
| [sources/](sources/) | Digests of documents read while researching, kept so a claim can be checked without re-fetching |

## Conventions

Files are named after the subject, not the date. A file that grows a decision
should end with one — the point where a note stops being research is when it
says what we are going to do.

---

*(Index above is maintained by hand; add a row when you add a file.)*
