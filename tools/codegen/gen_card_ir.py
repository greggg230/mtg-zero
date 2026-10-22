"""Generate the Rust ``serde`` types of the card IR from its schema.

Input: the schema JSON mtg-bot exports next to the IR
(``python -m mtg_bot.cli cards export-ir``; checked in at
``data/cards/powerlsv.schema.json``). Output: ``card_ir/src/generated.rs``.

    python tools/codegen/gen_card_ir.py \
        --schema data/cards/powerlsv.schema.json \
        --out card_ir/src/generated.rs

Mapping (see docs/card_ir.md):

* every Python class -> a struct in ``nodes`` with
  ``#[serde(deny_unknown_fields)]``; omitted (sparse) fields take the
  schema default;
* every category (Effect, ObjectPredicate, ..., one per record class) ->
  an enum in ``cats`` with ``#[serde(tag = "$t")]`` and one newtype
  variant per class, so an unknown tag is a deserialization error;
* a slot typed with several categories -> an enum in ``cats`` over all
  their classes; a slot typed ``int | <object>`` -> an untagged enum in
  ``unions``;
* Python enums -> ``enums`` (variants renamed to the Python values);
* ``count`` (int or ``"inf"``) -> ``support::Count``; a slot never seen
  with a value -> ``support::Never`` (deserializing one is an error).

The output is deterministic: same schema -> byte-identical file.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

RUST_KEYWORDS = {
    "as", "break", "const", "continue", "crate", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
    "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
    "super", "trait", "true", "type", "unsafe", "use", "where", "while",
    "async", "await", "dyn", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
}


def camel(name: str) -> str:
    """``cube_walkers.IsPermanent`` -> ``CubeWalkersIsPermanent``;
    ``BEGINNING_OF_UPKEEP`` -> ``BeginningOfUpkeep``; ``_Controller`` ->
    ``Controller``."""
    parts = re.split(r"[._]+", name)
    out = []
    for p in parts:
        if not p:
            continue
        if p.isupper() or p.islower():
            out.append(p[:1].upper() + p[1:].lower())
        else:
            out.append(p[:1].upper() + p[1:])
    s = "".join(out)
    if not s or s[0].isdigit():
        s = "V" + s
    if s in RUST_KEYWORDS:  # ``Self``
        s += "_"
    return s


def field_ident(name: str) -> str:
    return f"r#{name}" if name in RUST_KEYWORDS else name


class Gen:
    def __init__(self, schema: dict, schema_sha: str):
        self.schema = schema
        self.sha = schema_sha
        self.categories: dict[str, list[str]] = schema["categories"]
        self.classes: dict[str, dict] = schema["classes"]
        self.enums: dict[str, dict] = schema["enums"]
        self.struct_name = {tag: camel(tag) for tag in self.classes}
        if len(set(self.struct_name.values())) != len(self.struct_name):
            raise SystemExit("struct name collision")
        self.enum_name = {tag: camel(tag) for tag in self.enums}
        self.ref_enums: dict[tuple[str, ...], str] = {}
        self.unions: dict[str, tuple[str, list[tuple[str, dict]]]] = {}
        self.defaults: list[str] = []

    # ---- types ------------------------------------------------------------
    def ref_enum(self, cats: list[str]) -> str:
        key = tuple(sorted(cats))
        if key not in self.ref_enums:
            self.ref_enums[key] = "Or".join(camel(c) for c in key)
        return self.ref_enums[key]

    def union(self, t: dict) -> str:
        arms = []
        for x in t["items"]:
            arms.append((self.arm_name(x), x))
        name = "Or".join(a for a, _ in arms)
        self.unions.setdefault(name, (name, arms))
        return name

    def arm_name(self, t: dict) -> str:
        k = t["t"]
        if k == "ref":
            return self.ref_enum(t["categories"])
        if k == "enum":
            return self.enum_name[t["enum"]]
        return {"int": "Int", "float": "Float", "str": "Str", "bool": "Bool",
                "count": "Count", "list": "List", "set": "Set", "tuple": "Tuple",
                "keyword": "Keyword"}[k]

    def rust(self, t: dict, boxed: bool = True) -> str:
        k = t["t"]
        if k == "bool":
            return "bool"
        if k == "int":
            return "i64"
        if k == "float":
            return "f64"
        if k == "str":
            return "String"
        if k == "count":
            return "Count"
        if k == "never":
            return "Never"
        if k == "keyword":
            r = "cats::Keyword"
            return f"Box<{r}>" if boxed else r
        if k == "enum":
            return f"enums::{self.enum_name[t['enum']]}"
        if k == "ref":
            r = f"cats::{self.ref_enum(t['categories'])}"
            return f"Box<{r}>" if boxed else r
        if k == "opt":
            return f"Option<{self.rust(t['of'], True)}>"
        if k in ("list", "set"):
            return f"Vec<{self.rust(t['of'], False)}>"
        if k == "tuple":
            items = [self.rust(x, True) for x in t["items"]]
            return "(" + ", ".join(items) + ("," if len(items) == 1 else "") + ")"
        if k == "union":
            return f"unions::{self.union(t)}"
        raise SystemExit(f"unknown IR type {t}")

    @staticmethod
    def rust_default_json(t: dict):
        """JSON of Rust's ``Default`` for the type, or a sentinel."""
        k = t["t"]
        return {"bool": False, "int": 0, "float": 0, "str": "", "list": [],
                "set": [], "opt": None}.get(k, ...)

    # ---- emit -------------------------------------------------------------
    def emit(self) -> str:
        # Walk all fields first so every ref enum / union is registered.
        struct_blocks = []
        for tag in sorted(self.classes):
            struct_blocks.append(self.emit_struct(tag))
        for cat in self.categories:
            self.ref_enum([cat])
        out = [
            "// @generated by tools/codegen/gen_card_ir.py -- do not edit.",
            f"// Schema: ir_version {self.schema['ir_version']}, sha256 {self.sha}",
            "#![allow(clippy::all, clippy::pedantic, non_camel_case_types, non_snake_case, unused_imports, dead_code)]",
            "",
            "use crate::support::{Count, Never};",
            "",
            f'pub const SCHEMA_SHA256: &str = "{self.sha}";',
            f"pub const IR_VERSION: u32 = {self.schema['ir_version']};",
            "",
            "/// Python enums of the card model (variants renamed to the Python values).",
            "pub mod enums {",
            "    use serde::Deserialize;",
        ]
        for tag in sorted(self.enums):
            e = self.enums[tag]
            out.append("")
            out.append(f"    /// `{e['module']}.{e['qualname']}`")
            out.append("    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]")
            out.append(f"    pub enum {self.enum_name[tag]} {{")
            seen = set()
            for m in e["members"]:
                v = camel(m["name"])
                if v in seen:
                    raise SystemExit(f"variant collision {tag}.{v}")
                seen.add(v)
                out.append(f'        #[serde(rename = "{m["value"]}")]')
                out.append(f"        {v},")
            out.append("    }")
        out.append("}")
        out.append("")
        out.append("/// One struct per Python class (`$t` tag in the doc comment).")
        out.append("pub mod nodes {")
        out.append("    use super::{cats, enums, unions, defaults};")
        out.append("    use crate::support::{Count, Never};")
        out.append("    use serde::Deserialize;")
        out.extend(struct_blocks)
        out.append("}")
        out.append("")
        out.append("/// Tagged enums: one per category, plus one per multi-category slot type.")
        out.append("pub mod cats {")
        out.append("    use super::nodes;")
        out.append("    use serde::Deserialize;")
        for key in sorted(self.ref_enums, key=lambda k: self.ref_enums[k]):
            name = self.ref_enums[key]
            tags = sorted({t for c in key for t in self.categories.get(c, [])})
            if not tags:
                raise SystemExit(f"empty category set {key}")
            out.append("")
            out.append(f"    /// Categories: {', '.join(key)}")
            out.append("    #[derive(Debug, Clone, PartialEq, Deserialize)]")
            out.append('    #[serde(tag = "$t")]')
            out.append(f"    pub enum {name} {{")
            for tag in tags:
                out.append(f'        #[serde(rename = "{tag}")]')
                out.append(f"        {self.struct_name[tag]}(nodes::{self.struct_name[tag]}),")
            out.append("    }")
            out.append("")
            out.append(f"    impl {name} {{")
            out.append("        /// The variant's `$t` tag.")
            out.append("        pub fn tag(&self) -> &'static str {")
            out.append("            match self {")
            for tag in tags:
                out.append(f'                Self::{self.struct_name[tag]}(_) => "{tag}",')
            out.append("            }")
            out.append("        }")
            out.append("    }")
        out.append("}")
        out.append("")
        out.append("/// Untagged unions of a scalar and an object (e.g. `int | Formula`).")
        out.append("pub mod unions {")
        out.append("    use super::{cats, enums};")
        out.append("    use crate::support::{Count, Never};")
        out.append("    use serde::Deserialize;")
        for name in sorted(self.unions):
            _, arms = self.unions[name]
            out.append("")
            out.append("    #[derive(Debug, Clone, PartialEq, Deserialize)]")
            out.append("    #[serde(untagged)]")
            out.append(f"    pub enum {name} {{")
            for arm, t in arms:
                out.append(f"        {arm}({self.rust(t, True)}),")
            out.append("    }")
        out.append("}")
        out.append("")
        out.append("/// Non-trivial field defaults (sparse IR omits fields equal to them).")
        out.append("pub mod defaults {")
        out.append("    use super::{cats, enums, unions};")
        out.append("    use crate::support::{Count, Never};")
        out.extend(self.defaults)
        out.append("}")
        out.append("")
        out.append("/// `$t` tag -> category, for every class in the schema.")
        out.append("pub static TAG_CATEGORY: &[(&str, &str)] = &[")
        for tag in sorted(self.classes):
            out.append(f'    ("{tag}", "{self.classes[tag]["category"]}"),')
        out.append("];")
        out.append("")
        return "\n".join(out)

    def emit_struct(self, tag: str) -> str:
        c = self.classes[tag]
        name = self.struct_name[tag]
        lines = [
            "",
            f"    /// `{c['module']}.{c['qualname']}` (`$t` = `{tag}`, category `{c['category']}`)",
            "    #[derive(Debug, Clone, PartialEq, Deserialize)]",
            "    #[serde(deny_unknown_fields)]",
            f"    pub struct {name} {{",
        ]
        for f in c["fields"]:
            ty = self.rust(f["type"], True)
            ident = field_ident(f["name"])
            if not f["required"]:
                d = f["default"]
                rd = self.rust_default_json(f["type"])
                if rd is not ... and d == rd and not (isinstance(d, bool) != isinstance(rd, bool)):
                    lines.append("        #[serde(default)]")
                else:
                    fn = f"d_{name}_{f['name']}"
                    blob = json.dumps(d, sort_keys=True, ensure_ascii=False)
                    self.defaults.append(
                        f"    pub fn {fn}() -> {ty} {{\n"
                        f'        serde_json::from_str(r##"{blob}"##).expect("schema default")\n'
                        f"    }}"
                    )
                    lines.append(f'        #[serde(default = "defaults::{fn}")]')
            lines.append(f"        pub {ident}: {ty},")
        lines.append("    }")
        return "\n".join(lines)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--schema", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--check", action="store_true", help="fail if --out is stale")
    args = ap.parse_args(argv)
    raw = args.schema.read_text(encoding="utf-8")
    schema = json.loads(raw)
    if schema.get("format") != "mtg-bot-card-ir-schema":
        raise SystemExit(f"{args.schema}: not a card IR schema")
    canon = json.dumps(schema, sort_keys=True, separators=(",", ":"))
    sha = hashlib.sha256(canon.encode()).hexdigest()
    code = Gen(schema, sha).emit()
    if args.check:
        current = args.out.read_text(encoding="utf-8") if args.out.exists() else ""
        if current != code:
            print(f"{args.out} is stale; rerun without --check", file=sys.stderr)
            return 1
        return 0
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(code, encoding="utf-8", newline="\n")
    print(f"wrote {args.out} ({len(code.splitlines())} lines, schema {sha[:12]})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
