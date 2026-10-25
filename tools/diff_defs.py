"""Diff two dumps of the lowered card definitions (ENGINE.md 15).

    SUPPORT_DEFS=old.txt cargo test --release -p engine --test support cube_support_report
    (change the IR or the lowering)
    SUPPORT_DEFS=new.txt cargo test --release -p engine --test support cube_support_report
    python tools/diff_defs.py old.txt new.txt [--show]

A dump is ``#id kind name`` lines, each followed by that definition's
``CardDef`` (Debug, pretty). Definition ids of faces and tokens move when
another card's support changes (faces come after the tokens, the tokens
after the supported cards), so the references to them (``back``,
``front``, ``adventure``, ``bestow``, ``prepare``, ``halves``, ``levels``)
are written as the name they point to before comparing; token indices
(``token``, ``army``, ``emblem``: the n-th token definition) likewise.
Prints the definitions added, removed and changed (by kind and name).
"""

from __future__ import annotations

import difflib
import re
import sys

HEAD = re.compile(r"^#(\d+) (card|face|token) (.*)$", re.M)
SUBTYPE = re.compile(r"^@subtype (\d+) (.*)$", re.M)
ONE_ID = re.compile(r"(\b(?:back|front|adventure|bestow|prepare): Some\(\n\s*)(\d+)(,\n)")
HALVES = re.compile(r"(\bhalves: Some\(\n\s*\(\n\s*)(\d+)(,\n\s*)(\d+)(,\n)")
LEVELS = re.compile(r"(\blevels: \[\n)((?:\s*\d+,\n)+)")
TOKEN = re.compile(r"(\b(?:token|army|emblem): )(\d+)(,)")
# Interned subtype (and counter-name) ids.
SUB_ONE = re.compile(r"(\b(?:Subtype|Named)\(\n\s*)(\d+)(,\n)")
SUB_SOME = re.compile(r"(\b(?:attacked_subtype|blocks_only_subtype): Some\(\n\s*)(\d+)(,\n)")
SUB_FIELD = re.compile(r"(\bsubtype: )(\d+)(,)")
SUB_LIST = re.compile(r"(\bsubtypes: \[\n)((?:\s*\d+,\n)+)")


def load(path: str) -> tuple[dict[tuple[str, str], str], dict[int, str]]:
    text = open(path, encoding="utf-8").read()
    subs = {int(m.group(1)): m.group(2) for m in SUBTYPE.finditer(text)}
    heads = list(HEAD.finditer(text))
    names: dict[int, str] = {}
    kinds: dict[int, str] = {}
    for h in heads:
        names[int(h.group(1))] = h.group(3)
        kinds[int(h.group(1))] = h.group(2)
    tokens = sorted(i for i, k in kinds.items() if k == "token")
    token_base = tokens[0] if tokens else 0

    def name_of(i: int) -> str:
        return f"<{kinds.get(i, '?')} {names.get(i, '?')}>"

    def norm(body: str) -> str:
        body = ONE_ID.sub(lambda m: m.group(1) + name_of(int(m.group(2))) + m.group(3), body)
        body = HALVES.sub(
            lambda m: m.group(1) + name_of(int(m.group(2))) + m.group(3) + name_of(int(m.group(4))) + m.group(5), body
        )
        body = LEVELS.sub(
            lambda m: m.group(1)
            + "".join(re.sub(r"\d+", lambda d: name_of(int(d.group(0))), ln) + "\n" for ln in m.group(2).splitlines()),
            body,
        )
        body = TOKEN.sub(lambda m: m.group(1) + name_of(token_base + int(m.group(2))) + m.group(3), body)

        def sub(i: str) -> str:
            return f"<subtype {subs.get(int(i), '?' + i)}>"

        body = SUB_ONE.sub(lambda m: m.group(1) + sub(m.group(2)) + m.group(3), body)
        body = SUB_SOME.sub(lambda m: m.group(1) + sub(m.group(2)) + m.group(3), body)
        body = SUB_FIELD.sub(lambda m: m.group(1) + sub(m.group(2)) + m.group(3), body)
        body = SUB_LIST.sub(
            lambda m: m.group(1) + "".join(re.sub(r"\d+", lambda d: sub(d.group(0)), ln) + "\n" for ln in m.group(2).splitlines()),
            body,
        )
        return body

    defs: dict[tuple[str, str], str] = {}
    for k, h in enumerate(heads):
        end = heads[k + 1].start() if k + 1 < len(heads) else len(text)
        key = (h.group(2), h.group(3))
        body = norm(text[h.end() : end])
        # (Two faces or tokens may share a name: keep both.)
        while key in defs:
            key = (key[0], key[1] + "'")
        defs[key] = body
    return defs, names


def main() -> int:
    old, _ = load(sys.argv[1])
    new, _ = load(sys.argv[2])
    show = "--show" in sys.argv
    for k in sorted(set(old) - set(new)):
        print("REMOVED", *k)
    for k in sorted(set(new) - set(old)):
        print("ADDED  ", *k)
    changed = [k for k in sorted(set(old) & set(new)) if old[k] != new[k]]
    for k in changed:
        print("CHANGED", *k)
        if show:
            diff = difflib.unified_diff(old[k].splitlines(), new[k].splitlines(), "old", "new", n=2, lineterm="")
            for line in diff:
                print("   ", line)
    print(f"{len(set(old) - set(new))} removed, {len(set(new) - set(old))} added, {len(changed)} changed, "
          f"{len(set(old) & set(new)) - len(changed)} unchanged")
    return 0


if __name__ == "__main__":
    sys.exit(main())
