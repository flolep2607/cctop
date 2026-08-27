#!/usr/bin/env python3
"""Read a harness transcript the way cctop has to: by shape, not by hope.

cctop reads seven harnesses' JSONL, none of which documents its format, and the
usual question is one of:

    what fields does this file actually contain?
    which record carries <this value>, and under what key?
    what changed between a session that works and one that does not?

Every one of those was answered with a throwaway `python3 -c` loop at least
twice while writing cctop. This is that loop, kept.

    transcript.py fields <file>            every key path, with counts and a sample
    transcript.py find <file> <text>       which key paths hold that text
    transcript.py types <file>             record types and how many of each
    transcript.py tail <file> [n]          the last n records, minus the noise
    transcript.py diff <a> <b>             key paths in one file and not the other

`fields` is the one to reach for first. It is how `session_id` — the field that
records the id a resumed session was launched from, distinct from `sessionId`
and the whole reason a forked session can be matched to its process — was found
in the first place.
"""

import collections
import json
import sys
from pathlib import Path

# Long free text drowns the output and says nothing about shape.
NOISY = {"content", "text", "stdout", "stderr", "snapshot", "toolUseResult"}


def records(path: Path):
    with path.open(errors="replace") as fh:
        for n, line in enumerate(fh, 1):
            line = line.strip()
            if not line:
                continue
            try:
                yield n, json.loads(line)
            except json.JSONDecodeError:
                print(f"  line {n}: not JSON", file=sys.stderr)


def walk(obj, path=""):
    """Every leaf, as `a.b[].c` → value."""
    if isinstance(obj, dict):
        for k, v in obj.items():
            yield from walk(v, f"{path}.{k}" if path else k)
    elif isinstance(obj, list):
        for v in obj[:4]:
            yield from walk(v, f"{path}[]")
    else:
        yield path, obj


def cmd_fields(path: Path) -> int:
    seen: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    first_line: dict[str, int] = {}
    for n, rec in records(path):
        for key, value in walk(rec):
            seen[key][type(value).__name__] += 1
            first_line.setdefault(key, n)
    # Capped: one deeply nested key path would otherwise set the column width
    # for every line and push the useful half off the screen.
    width = min(max((len(k) for k in seen), default=0), 52)
    for key in sorted(seen):
        kinds = ", ".join(f"{t}×{c}" for t, c in seen[key].most_common())
        print(f"{key:<{width}}  {kinds:<22} first at line {first_line[key]}")
    return 0


def cmd_find(path: Path, needle: str) -> int:
    hits: dict[str, list] = collections.defaultdict(list)
    for n, rec in records(path):
        for key, value in walk(rec):
            if isinstance(value, str) and needle in value:
                if len(hits[key]) < 3:
                    hits[key].append((n, value[:120]))
    if not hits:
        print(f"{needle!r} appears in no value in {path.name}")
        return 1
    for key, examples in sorted(hits.items()):
        print(f"{key}")
        for n, value in examples:
            print(f"  line {n}: {value}")
    return 0


def cmd_types(path: Path) -> int:
    kinds = collections.Counter()
    for _, rec in records(path):
        kinds[rec.get("type", "(no type)")] += 1
    for kind, count in kinds.most_common():
        print(f"{count:>6}  {kind}")
    return 0


def cmd_tail(path: Path, count: str = "5") -> int:
    keep = [rec for _, rec in records(path)][-int(count) :]
    for rec in keep:
        trimmed = {k: v for k, v in rec.items() if k not in NOISY}
        print(json.dumps(trimmed)[:600])
    return 0


def cmd_diff(a: Path, b: Path) -> int:
    def keys(path: Path) -> set[str]:
        return {key for _, rec in records(path) for key, _ in walk(rec)}

    left, right = keys(a), keys(b)
    for key in sorted(left - right):
        print(f"- {key}   (only in {a.name})")
    for key in sorted(right - left):
        print(f"+ {key}   (only in {b.name})")
    if left == right:
        print("the same key paths in both")
    return 0


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    verb, path = sys.argv[1], Path(sys.argv[2])
    if not path.exists():
        print(f"no such file: {path}", file=sys.stderr)
        return 1
    rest = sys.argv[3:]
    match verb:
        case "fields":
            return cmd_fields(path)
        case "find":
            return cmd_find(path, rest[0]) if rest else 2
        case "types":
            return cmd_types(path)
        case "tail":
            return cmd_tail(path, *rest[:1])
        case "diff":
            return cmd_diff(path, Path(rest[0])) if rest else 2
        case _:
            print(__doc__)
            return 2


if __name__ == "__main__":
    raise SystemExit(main())
