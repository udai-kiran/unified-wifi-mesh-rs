"""Tests for the em-dm declaration inventory generator."""
# SPDX-License-Identifier: Apache-2.0

import csv
import importlib.machinery
import importlib.util
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

REPO = Path(__file__).resolve().parent.parent
SCRIPT = REPO / "tools/gen-dm-inventory"
DATA = REPO / "crates/em-dm/tests/data"
loader = importlib.machinery.SourceFileLoader("dm_inventory_generator", str(SCRIPT))
spec = importlib.util.spec_from_loader(loader.name, loader)
gen = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = gen
loader.exec_module(gen)


def policy_fixture(leaf="mac_addr_t", nested_field="sta_mac"):
    base = gen.Source(
        "typedef struct { "
        + leaf
        + " "
        + nested_field
        + "; } em_client_filters_cfg_t;\n"
        "typedef struct { em_client_filters_cfg_t client_filters; } em_policy_t;\n"
        "typedef enum { vendor_ext_attr_id_haul_type = 3 } vendor_ext_attr_id_t;\n",
        "test.h",
    )
    external = gen.Source("typedef unsigned char mac_addr_t[6];\n", "external.h")
    decl = external.get("mac_addr_t")
    row = {
        "name": "mac_addr_t",
        "kind": "alias",
        "definition": decl.definition,
        "repository": "OneWifi",
        "revision": gen.ONEWIFI,
        "commit_date": gen.PINS["OneWifi"][1],
        "source": "external.h:1",
    }
    sources = {("OneWifi", "external.h"): external}
    manifest = gen.validate_manifest([row], sources)
    walker = gen.Walker(base, sources, manifest)
    return walker


def classification_row(name, line, category="stored-native"):
    return {
        "type": name,
        "em_base.h_line": str(line) if line else "-",
        "category": category,
        "proposed_owner": "em-dm",
        "reason": "fixture",
        "stored_evidence": "test.h:1",
        "wire_evidence": "-",
    }


def compiler_members(object_path):
    """Extract typedef-to-struct fields in declaration order from compiler DWARF."""
    result = subprocess.run(
        ["readelf", "-W", "--debug-dump=info", str(object_path)],
        text=True,
        capture_output=True,
        check=True,
    )
    entries, stack, current = {}, {}, None
    for line in result.stdout.splitlines():
        entry = re.match(
            r"\s*<(\d+)><([0-9a-f]+)>: Abbrev Number: \d+ \(DW_TAG_(\w+)\)", line
        )
        if entry:
            depth, offset, tag = int(entry[1]), int(entry[2], 16), entry[3]
            current = {"tag": tag, "children": [], "attrs": {}}
            entries[offset] = current
            if depth and depth - 1 in stack:
                stack[depth - 1]["children"].append(current)
            stack[depth] = current
            for level in list(stack):
                if level > depth:
                    del stack[level]
            continue
        if re.match(r"\s*<\d+><[0-9a-f]+>: Abbrev Number: 0", line):
            current = None
            continue
        attribute = re.match(r"\s*<[0-9a-f]+>\s+DW_AT_(\w+)\s*:\s*(.*)", line)
        if current and attribute:
            current["attrs"][attribute[1]] = attribute[2]

    def name(entry):
        value = entry["attrs"].get("name", "")
        indirect = re.search(r"\(offset: [^)]*\):\s*(.*)$", value)
        if indirect:
            return indirect[1]
        inline = re.match(r"\(string\)\s*(.*)$", value)
        return inline[1] if inline else value

    result = {}
    for entry in entries.values():
        if entry["tag"] != "typedef":
            continue
        reference = re.search(r"<0x([0-9a-f]+)>", entry["attrs"].get("type", ""))
        target = entries.get(int(reference[1], 16)) if reference else None
        if not target or target["tag"] != "structure_type":
            continue
        fields = []
        for child in target["children"]:
            if child["tag"] != "member":
                continue
            bit_size = child["attrs"].get("bit_size")
            size = int(bit_size.rsplit(" ", 1)[-1], 0) if bit_size else None
            fields.append((name(child), size))
        result[name(entry)] = fields
    return result


def assert_compiler_members(test, walker, actual):
    """Compare all reached struct members to the compiler's independent list."""
    for owner, decl in walker.reached.items():
        if decl is None or decl.kind != "struct":
            continue
        expected = [
            (row[1], None if row[5] == "-" else int(row[5]))
            for row in walker.rows
            if row[0] == owner
        ]
        test.assertIn(owner, actual, f"compiler omitted debug information for {owner}")
        test.assertEqual(
            actual[owner], expected, f"compiler field list differs for {owner}"
        )


class TraversalTests(unittest.TestCase):
    def test_policy_follows_nested_external_alias_to_its_elements(self):
        walker = policy_fixture()
        walker.walk("em_policy_t")
        rows = {(row[0], row[1]): row for row in walker.rows}
        self.assertEqual(
            rows[("em_policy_t", "client_filters")][2], "em_client_filters_cfg_t"
        )
        self.assertEqual(rows[("em_client_filters_cfg_t", "sta_mac")][2], "mac_addr_t")
        self.assertEqual(
            rows[("mac_addr_t", "<alias>")][2:5], ("unsigned char", "6", "6")
        )
        self.assertEqual(walker.used_external, {"mac_addr_t"})

    def test_unresolved_nested_dependency_fails(self):
        walker = policy_fixture(leaf="missing_mac_t")
        with self.assertRaisesRegex(gen.ContentError, "missing_mac_t"):
            walker.walk("em_policy_t")

    def test_missing_external_manifest_entry_fails(self):
        walker = policy_fixture()
        walker.manifest.clear()
        with self.assertRaisesRegex(gen.ContentError, "mac_addr_t"):
            walker.walk("em_policy_t")

    def test_manifest_definition_is_checked_against_pinned_source(self):
        walker = policy_fixture()
        row = dict(walker.manifest["mac_addr_t"])
        row["definition"] = "typedef unsigned char mac_addr_t[7];"
        with self.assertRaisesRegex(gen.ContentError, "does not match"):
            gen.validate_manifest([row], walker.sources)

    def test_duplicate_field_rejected(self):
        base = gen.Source("typedef struct { int x; int x; } record_t;", "test.h")
        with self.assertRaisesRegex(gen.ContentError, "duplicate field"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_missing_field_terminator_rejected(self):
        base = gen.Source("typedef struct { int x; int y } record_t;", "test.h")
        with self.assertRaisesRegex(gen.ContentError, "missing field terminator"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_missing_field_name_rejected(self):
        base = gen.Source("typedef struct { unsigned char; } record_t;", "test.h")
        with self.assertRaisesRegex(gen.ContentError, "missing or invalid field"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_unresolved_array_bound_rejected(self):
        base = gen.Source("typedef struct { char data[UNKNOWN]; } record_t;", "test.h")
        with self.assertRaisesRegex(gen.ContentError, "UNKNOWN"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_unparenthesized_macro_expression_is_rejected(self):
        for replacement in ("1 + 2", "(1) + (2)"):
            with self.subTest(replacement=replacement):
                base = gen.Source(
                    f"#define N {replacement}\n"
                    "typedef struct { char data[2 * N]; } record_t;",
                    "test.h",
                )
                with self.assertRaisesRegex(gen.ContentError, "N"):
                    gen.Walker(base, {}, {}).walk("record_t")

    def test_unparenthesized_macro_as_entire_bound_is_accepted(self):
        base = gen.Source(
            "#define N 1 + 2\ntypedef struct { char data[N]; } record_t;", "test.h"
        )
        walker = gen.Walker(base, {}, {})
        walker.walk("record_t")
        self.assertEqual(walker.rows[0][3:5], ("N", "3"))

    def test_unparenthesized_external_macro_as_entire_bound_is_accepted(self):
        base = gen.Source("typedef struct { char data[N]; } record_t;", "test.h")
        source = gen.Source("#define N 1 + 2\n", "external.h")
        row = {
            "name": "N",
            "kind": "constant",
            "definition": "1 + 2",
            "repository": "OneWifi",
            "revision": gen.ONEWIFI,
            "commit_date": gen.PINS["OneWifi"][1],
            "source": "external.h:1",
        }
        sources = {("OneWifi", "external.h"): source}
        walker = gen.Walker(base, sources, gen.validate_manifest([row], sources))
        walker.walk("record_t")
        self.assertEqual(walker.rows[0][3:5], ("N", "3"))

    def test_unparenthesized_macro_in_multiplication_is_rejected_after_cached_bound(
        self,
    ):
        base = gen.Source(
            "#define N 1 + 2\n"
            "typedef struct { char first[N]; char second[2 * N]; } record_t;",
            "test.h",
        )
        with self.assertRaisesRegex(gen.ContentError, "macro N"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_unparenthesized_macro_in_division_is_rejected(self):
        base = gen.Source(
            "#define N 1 + 2\ntypedef struct { char x[12 / N]; } record_t;", "test.h"
        )
        with self.assertRaisesRegex(gen.ContentError, "macro N"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_token_pasting_in_whole_bound_macro_is_rejected(self):
        base = gen.Source(
            "#define N 1 ## 2\ntypedef struct { char x[N]; } record_t;", "test.h"
        )
        with self.assertRaisesRegex(gen.ContentError, "macro N"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_python_only_integer_literal_in_whole_bound_macro_is_rejected(self):
        base = gen.Source(
            "#define N 1_000\ntypedef struct { char x[N]; } record_t;", "test.h"
        )
        with self.assertRaisesRegex(gen.ContentError, "macro N"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_increment_token_in_whole_bound_macro_is_rejected(self):
        base = gen.Source(
            "#define N 1++2\ntypedef struct { char x[N]; } record_t;", "test.h"
        )
        with self.assertRaisesRegex(gen.ContentError, "macro N"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_unparenthesized_macro_through_another_macro_is_rejected(self):
        for outer in ("N", "(2 * N)"):
            with self.subTest(outer=outer):
                base = gen.Source(
                    f"#define N 1 + 2\n#define M {outer}\n"
                    "typedef struct { char x[M]; } record_t;",
                    "test.h",
                )
                with self.assertRaisesRegex(gen.ContentError, "macro N"):
                    gen.Walker(base, {}, {}).walk("record_t")

    def test_unparenthesized_macro_outside_a_bound_is_rejected(self):
        for declaration, root in (
            ("typedef enum { X = N } mode_t;", "mode_t"),
            ("typedef struct { unsigned x:N; } record_t;", "record_t"),
        ):
            with self.subTest(declaration=declaration):
                base = gen.Source("#define N 1 + 2\n" + declaration, "test.h")
                with self.assertRaisesRegex(gen.ContentError, "macro N"):
                    gen.Walker(base, {}, {}).walk(root)

    def test_single_token_and_parenthesized_macro_replacements_are_accepted(self):
        base = gen.Source(
            "#define M 2\n#define N M\n#define P (1 + N)\n"
            "typedef struct { char data[2 * P]; } record_t;",
            "test.h",
        )
        walker = gen.Walker(base, {}, {})
        walker.walk("record_t")
        self.assertEqual(walker.rows[0][3:5], ("2 * P", "6"))

    def test_unparenthesized_external_macro_expression_is_rejected(self):
        base = gen.Source("typedef struct { char data[2 * N]; } record_t;", "test.h")
        source = gen.Source("#define N 1 + 2\n", "external.h")
        row = {
            "name": "N",
            "kind": "constant",
            "definition": "1 + 2",
            "repository": "OneWifi",
            "revision": gen.ONEWIFI,
            "commit_date": gen.PINS["OneWifi"][1],
            "source": "external.h:1",
        }
        sources = {("OneWifi", "external.h"): source}
        manifest = gen.validate_manifest([row], sources)
        with self.assertRaisesRegex(gen.ContentError, "N"):
            gen.Walker(base, sources, manifest).walk("record_t")

    def test_literal_in_parenthesized_macro_used_as_bound_is_rejected(self):
        for literal in ("'A'", '"abc"'):
            with self.subTest(literal=literal):
                base = gen.Source(
                    f"#define N ({literal} + 1)\n"
                    "typedef struct { char x[N]; } record_t;",
                    "test.h",
                )
                with self.assertRaisesRegex(gen.ContentError, "literal"):
                    gen.Walker(base, {}, {}).walk("record_t")

    def test_malformed_parenthesized_macro_names_its_source(self):
        base = gen.Source(
            "#define N (1 + )\ntypedef struct { char x[N]; } record_t;", "test.h"
        )
        with self.assertRaisesRegex(
            gen.ContentError, "macro N.*invalid integer expression"
        ):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_character_literal_in_enum_is_rejected(self):
        base = gen.Source("typedef enum { FIRST = 'A' + 1 } mode_t;", "test.h")
        with self.assertRaisesRegex(gen.ContentError, "literal"):
            gen.Walker(base, {}, {}).walk("mode_t")

    def test_character_literal_in_bound_is_rejected(self):
        base = gen.Source("typedef struct { char x['A' + 1]; } record_t;", "test.h")
        with self.assertRaisesRegex(gen.ContentError, "literal"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_string_literal_in_bound_is_rejected(self):
        base = gen.Source('typedef struct { char x["abc" + 1]; } record_t;', "test.h")
        with self.assertRaisesRegex(gen.ContentError, "literal"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_character_literal_in_bit_width_is_rejected(self):
        base = gen.Source("typedef struct { unsigned x:'A' + 1; } record_t;", "test.h")
        with self.assertRaisesRegex(gen.ContentError, "literal"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_directive_in_typedef_trailer_is_rejected(self):
        base = gen.Source(
            "typedef struct { int x; }\n#if 0\nrecord_t;\n#else\n"
            "alternate_t;\n#endif\n",
            "test.h",
        )
        with self.assertRaisesRegex(gen.ContentError, "directive"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_directive_in_alias_declaration_is_rejected(self):
        base = gen.Source(
            "typedef int\n#if 0\nrecord_t;\n#else\nalternate_t;\n#endif\n", "test.h"
        )
        with self.assertRaisesRegex(gen.ContentError, "directive"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_undefined_macro_is_rejected_even_after_use(self):
        for text in (
            "#define N 3\n#undef N\ntypedef struct { char x[N]; } record_t;\n",
            "#define N 3\ntypedef struct { char x[N]; } record_t;\n#undef N\n",
        ):
            with self.subTest(text=text):
                base = gen.Source(text, "test.h")
                with self.assertRaisesRegex(gen.ContentError, "N"):
                    gen.Walker(base, {}, {}).walk("record_t")

    def test_undefined_macro_in_another_read_header_is_rejected(self):
        base = gen.Source(
            "#define N 3\ntypedef struct { char x[N]; } record_t;\n", "test.h"
        )
        other = gen.Source("#undef N\n", "other.h")
        with self.assertRaisesRegex(gen.ContentError, "N"):
            gen.Walker(base, {("other", "other.h"): other}, {}).walk("record_t")

    def test_identical_macro_redefinitions_are_accepted_and_recorded(self):
        base = gen.Source(
            "#define N (1 + 2)\n#define N (1   + 2)\n"
            "typedef struct { char x[2 * N]; } record_t;\n",
            "test.h",
        )
        other = gen.Source("#define N (1\t+\t2)\n", "other.h")
        walker = gen.Walker(base, {("other", "other.h"): other}, {})
        walker.walk("record_t")
        self.assertEqual(walker.rows[0][3:5], ("2 * N", "6"))
        header = gen.format_inventory(walker)
        self.assertIn(
            "N: unified-wifi-mesh:test.h:1, unified-wifi-mesh:test.h:2, other:other.h:1",
            header,
        )

    def test_identical_external_macro_redefinitions_in_one_header_are_accepted(self):
        base = gen.Source("typedef struct { char x[N]; } record_t;", "test.h")
        other = gen.Source("#define N 3\n#define N  3\n", "other.h")
        row = {
            "name": "N",
            "kind": "constant",
            "definition": "3",
            "repository": "OneWifi",
            "revision": gen.ONEWIFI,
            "commit_date": gen.PINS["OneWifi"][1],
            "source": "other.h:1",
        }
        sources = {("OneWifi", "other.h"): other}
        walker = gen.Walker(base, sources, gen.validate_manifest([row], sources))
        walker.walk("record_t")
        self.assertEqual(walker.rows[0][4], "3")
        self.assertIn(
            "N: OneWifi:other.h:1, OneWifi:other.h:2", gen.format_inventory(walker)
        )

    def test_differing_macro_redefinitions_are_rejected(self):
        for replacement in ("4", "03", "3U"):
            with self.subTest(replacement=replacement):
                base = gen.Source(
                    "#define N 3\ntypedef struct { char x[N]; } record_t;\n", "test.h"
                )
                other = gen.Source(f"#define N {replacement}\n", "other.h")
                with self.assertRaisesRegex(gen.ContentError, "macro N"):
                    gen.Walker(base, {("other", "other.h"): other}, {}).walk("record_t")
                same_header = gen.Source(
                    f"#define N 3\n#define N {replacement}\n"
                    "typedef struct { char x[N]; } record_t;\n",
                    "test.h",
                )
                with self.assertRaisesRegex(gen.ContentError, "macro N"):
                    gen.Walker(same_header, {}, {}).walk("record_t")

                base = gen.Source("typedef struct { char x[N]; } record_t;\n", "test.h")
                other = gen.Source(f"#define N 3\n#define N {replacement}\n", "other.h")
                row = {
                    "name": "N",
                    "kind": "constant",
                    "definition": "3",
                    "repository": "OneWifi",
                    "revision": gen.ONEWIFI,
                    "commit_date": gen.PINS["OneWifi"][1],
                    "source": "other.h:1",
                }
                sources = {("OneWifi", "other.h"): other}
                manifest = gen.validate_manifest([row], sources)
                with self.assertRaisesRegex(gen.ContentError, "macro N"):
                    gen.Walker(base, sources, manifest).walk("record_t")

    def test_redefinition_preserves_c_token_separation(self):
        base = gen.Source(
            "#define N (1+2)\n#define N (1 + 2)\n"
            "typedef struct { char x[N]; } record_t;\n",
            "test.h",
        )
        with self.assertRaisesRegex(gen.ContentError, "macro N"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_undef_rejects_even_an_identically_redefined_macro(self):
        base = gen.Source(
            "#define N 3\n#define N 3\n"
            "typedef struct { char x[N]; } record_t;\n#undef N\n",
            "test.h",
        )
        with self.assertRaisesRegex(gen.ContentError, "macro N"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_continued_referenced_macro_is_rejected(self):
        base = gen.Source(
            "#define N(x) x \\\n+1\n#define N(x) x \\\n+2\n"
            "typedef enum { N = 3 } mode_t;\n"
            "typedef struct { char x[N]; } record_t;\n",
            "test.h",
        )
        with self.assertRaisesRegex(gen.ContentError, "macro N"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_function_like_redefinition_is_rejected(self):
        base = gen.Source(
            "#define N 3\n#define N(x) x\ntypedef struct { char x[N]; } record_t;\n",
            "test.h",
        )
        with self.assertRaisesRegex(gen.ContentError, "N"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_undefined_external_type_macro_is_rejected(self):
        base = gen.Source("typedef struct { BYTE x; } record_t;", "test.h")
        source = gen.Source(
            "#ifndef BYTE\n#define BYTE unsigned char\n#endif\n#undef BYTE\n",
            "external.h",
        )
        row = {
            "name": "BYTE",
            "kind": "type-macro",
            "definition": "unsigned char",
            "repository": "OneWifi",
            "revision": gen.ONEWIFI,
            "commit_date": gen.PINS["OneWifi"][1],
            "source": "external.h:2",
        }
        sources = {("OneWifi", "external.h"): source}
        manifest = gen.validate_manifest([row], sources)
        with self.assertRaisesRegex(gen.ContentError, "BYTE"):
            gen.Walker(base, sources, manifest).walk("record_t")

    def test_nonterminal_zero_length_member_is_rejected(self):
        base = gen.Source(
            "typedef struct { char tail[0]; int after; } record_t;", "test.h"
        )
        with self.assertRaisesRegex(gen.ContentError, "tail"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_enum_used_only_for_a_bound_is_emitted_and_requires_classification(self):
        base = gen.Source(
            "typedef enum { WIDTH = 3, NEXT } bound_t;\n"
            "typedef struct { char x[WIDTH]; } record_t;\n",
            "test.h",
        )
        walker = gen.Walker(base, {}, {})
        walker.walk("record_t")
        self.assertIn("bound_t", walker.reached)
        enums = [row for row in walker.rows if row[0] == "bound_t"]
        self.assertEqual(
            [(row[1], row[7]) for row in enums], [("WIDTH", "3"), ("NEXT", "4")]
        )
        walker.walk("bound_t")
        self.assertEqual(sum(row[0] == "bound_t" for row in walker.rows), 2)
        rows = [
            classification_row(name, base.get(name).line if base.get(name) else None)
            for name in walker.reached
            if name != "bound_t"
        ]
        with self.assertRaisesRegex(gen.ContentError, "bound_t"):
            gen.validate_classification(rows, walker, {})

    def test_external_enum_used_only_for_a_bound_is_manifested_and_emitted(self):
        base = gen.Source("typedef struct { char x[WIDTH]; } record_t;\n", "test.h")
        source = gen.Source("typedef enum { WIDTH = 3, NEXT } bound_t;\n", "external.h")
        decl = source.get("bound_t")
        row = {
            "name": "bound_t",
            "kind": "enum",
            "definition": decl.definition,
            "repository": "OneWifi",
            "revision": gen.ONEWIFI,
            "commit_date": gen.PINS["OneWifi"][1],
            "source": "external.h:1",
        }
        sources = {("OneWifi", "external.h"): source}
        manifest = gen.validate_manifest([row], sources)
        walker = gen.Walker(base, sources, manifest)
        walker.walk("record_t")
        self.assertIn("bound_t", walker.reached)
        self.assertEqual(walker.used_external, {"bound_t"})
        self.assertEqual(
            [(row[1], row[7]) for row in walker.rows if row[0] == "bound_t"],
            [("WIDTH", "3"), ("NEXT", "4")],
        )

    def test_conditional_field_is_not_silently_dropped(self):
        base = gen.Source(
            "typedef struct {\n#ifdef FLAG\n int x;\n#endif\n int y;\n} record_t;\n",
            "test.h",
        )
        with self.assertRaisesRegex(gen.ContentError, "directive"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_conditional_declaration_is_not_silently_selected(self):
        base = gen.Source(
            "#if FLAG\ntypedef struct { int x; } record_t;\n#endif\n", "test.h"
        )
        with self.assertRaisesRegex(gen.ContentError, "conditional context"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_array_dimensions_pointer_bit_field_and_enum_values(self):
        base = gen.Source(
            "#define N (2 + 1)\n"
            "typedef enum { FIRST = 0x10, NEXT, LAST = NEXT + 2 } mode_t;\n"
            "typedef struct { mode_t mode; unsigned char bits:3; "
            "char *ptr; int arr[N][2]; char tail[0]; } record_t;\n",
            "test.h",
        )
        walker = gen.Walker(base, {}, {})
        walker.walk("record_t")
        rows = {(row[0], row[1]): row for row in walker.rows}
        self.assertEqual(rows[("record_t", "mode")][6], "enum")
        self.assertEqual(rows[("record_t", "bits")][5:7], ("3", "bit-field"))
        self.assertEqual(rows[("record_t", "ptr")][6], "pointer")
        self.assertEqual(rows[("record_t", "arr")][3:5], ("N;2", "3;2"))
        self.assertEqual(rows[("record_t", "tail")][6], "wire-tail")
        self.assertEqual(
            [rows[("mode_t", x)][7] for x in ("FIRST", "NEXT", "LAST")],
            ["16", "17", "19"],
        )

    def test_malformed_enum_does_not_produce_partial_values(self):
        base = gen.Source("typedef enum { A = 1, ???, B } mode_t;", "test.h")
        with self.assertRaisesRegex(gen.ContentError, "malformed enum"):
            gen.Walker(base, {}, {}).walk("mode_t")

    def test_unsigned_underflow_is_not_evaluated_as_python_integer(self):
        base = gen.Source("typedef struct { char x[1U - 2U]; } record_t;", "test.h")
        with self.assertRaisesRegex(gen.ContentError, "nonnegative int range"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_oversized_bit_field_is_rejected(self):
        base = gen.Source("typedef struct { unsigned char x:9; } record_t;", "test.h")
        with self.assertRaisesRegex(gen.ContentError, "bit-field"):
            gen.Walker(base, {}, {}).walk("record_t")

    def test_duplicate_enumerators_across_reached_enums_are_rejected(self):
        base = gen.Source(
            "typedef enum { SAME = 1 } first_t;\n"
            "typedef enum { SAME = 1 } second_t;\n"
            "typedef struct { first_t a; second_t b; } record_t;\n",
            "test.h",
        )
        with self.assertRaisesRegex(gen.ContentError, "duplicate enumerator"):
            gen.Walker(base, {}, {}).walk("record_t")


class ClassificationTests(unittest.TestCase):
    def setUp(self):
        self.walker = policy_fixture()
        self.walker.walk("em_policy_t")
        self.rows = [
            classification_row(
                name,
                self.walker.base.get(name).line if self.walker.base.get(name) else None,
            )
            for name in self.walker.reached
        ]
        self.coverage = gen.coverage_rows(
            "| Line | Kind | Upstream type | Owner | Note | Status |\n"
            "|---|---|---|---|---|---|\n"
            "| 3 | enum | `vendor_ext_attr_id_t` | em-dm | wire | planned |\n"
        )

    def test_coverage_only_vendor_enum_is_required(self):
        with self.assertRaisesRegex(gen.ContentError, "vendor_ext_attr_id_t"):
            gen.validate_classification(self.rows, self.walker, self.coverage)
        self.rows.append(classification_row("vendor_ext_attr_id_t", 3, "wire-only"))
        counts = gen.validate_classification(self.rows, self.walker, self.coverage)
        self.assertEqual(counts["wire-only"], 1)

    def test_reassigned_row_needs_matching_coverage_owner(self):
        kept = next(row for row in self.rows if row["em_base.h_line"] != "-")
        moved = {}
        coverage = gen.coverage_rows(
            "| Line | Kind | Upstream type | Owner | Note | Status |\n"
            "|---|---|---|---|---|---|\n"
            "| 3 | enum | `vendor_ext_attr_id_t` | em-msg | wire | planned |\n"
            f"| {kept['em_base.h_line']} | struct | `{kept['type']}` | em-dm | stored | planned |\n",
            moved)
        self.assertEqual(moved, {"vendor_ext_attr_id_t": (3, "em-msg")})
        row = classification_row("vendor_ext_attr_id_t", 3, "wire-only")
        row["proposed_owner"] = "em-msg"
        self.rows.append(row)
        counts = gen.validate_classification(self.rows, self.walker, coverage, moved)
        self.assertEqual(counts["wire-only"], 1)
        with self.assertRaisesRegex(gen.ContentError, "extra .*vendor_ext_attr_id_t"):
            gen.validate_classification(self.rows, self.walker, coverage)
        row["proposed_owner"] = "em-core"
        with self.assertRaisesRegex(gen.ContentError, "extra .*vendor_ext_attr_id_t"):
            gen.validate_classification(self.rows, self.walker, coverage, moved)

    def test_unknown_category_is_rejected(self):
        self.rows.append(classification_row("vendor_ext_attr_id_t", 3, "wire-only"))
        self.rows[0]["category"] = "unreviewed"
        with self.assertRaisesRegex(gen.ContentError, "unknown category"):
            gen.validate_classification(self.rows, self.walker, self.coverage)

    def test_duplicate_classification_is_rejected(self):
        self.rows.append(classification_row("vendor_ext_attr_id_t", 3, "wire-only"))
        self.rows.append(dict(self.rows[0]))
        with self.assertRaisesRegex(gen.ContentError, "duplicate type"):
            gen.validate_classification(self.rows, self.walker, self.coverage)

    def test_formatted_coverage_owner_cannot_hide_a_row(self):
        coverage = gen.coverage_rows(
            "| Line | Kind | Upstream type | Owner | Note | Status |\n"
            "|---|---|---|---|---|---|\n"
            "| 3 | enum | `vendor_ext_attr_id_t` | **em-dm** | wire | planned |\n"
        )
        self.assertEqual(coverage, {"vendor_ext_attr_id_t": 3})
        with self.assertRaisesRegex(gen.ContentError, "vendor_ext_attr_id_t"):
            gen.validate_classification(self.rows, self.walker, coverage)

    def test_malformed_coverage_row_is_rejected(self):
        for bad in (
            "| 3 | enum | vendor_ext_attr_id_t | em-dm | wire | planned |",
            "| 3 | enum | `vendor_ext_attr_id_t` | em-dm | wire |",
            "3 | enum | `vendor_ext_attr_id_t` | em-dm | wire | planned |",
        ):
            with self.subTest(bad=bad):
                text = (
                    "| Line | Kind | Upstream type | Owner | Note | Status |\n"
                    "|---|---|---|---|---|---|\n"
                    "| 1 | enum | `other_t` | em-dm | note | planned |\n" + bad + "\n"
                )
                with self.assertRaisesRegex(gen.ContentError, "coverage"):
                    gen.coverage_rows(text)

    def test_reassigned_coverage_row_requires_destination(self):
        row = classification_row("vendor_ext_attr_id_t", 3, "wire-only")
        row["proposed_owner"] = "-"
        with self.assertRaisesRegex(gen.ContentError, "destination"):
            gen.validate_classification(self.rows + [row], self.walker, self.coverage)

    def test_every_classification_row_requires_a_reason(self):
        self.rows.append(classification_row("vendor_ext_attr_id_t", 3, "wire-only"))
        for name in ("vendor_ext_attr_id_t", "em_policy_t"):
            with self.subTest(name=name):
                rows = [dict(row) for row in self.rows]
                next(row for row in rows if row["type"] == name)["reason"] = "-"
                with self.assertRaisesRegex(gen.ContentError, "reason"):
                    gen.validate_classification(rows, self.walker, self.coverage)


class CommandTests(unittest.TestCase):
    def run_generator(self, *args):
        return subprocess.run(
            [sys.executable, str(SCRIPT), *map(str, args)],
            cwd=REPO,
            text=True,
            capture_output=True,
            check=False,
        )

    def test_checked_in_inventory_matches_the_pins(self):
        result = self.run_generator("--check")
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)

    def test_check_fails_for_stale_fixture(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "stale.tsv"
            path.write_text("stale\n", encoding="utf-8")
            result = self.run_generator("--check", "--out", path)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("inventory fixture is stale", result.stderr)
            self.assertEqual(path.read_text(encoding="utf-8"), "stale\n")

    def test_check_detects_one_missing_nested_field(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "inventory.tsv"
            original = (DATA / "dm_inventory.tsv").read_text(encoding="utf-8")
            lines = original.splitlines(keepends=True)
            remaining = [
                line
                for line in lines
                if not line.startswith("em_client_filters_cfg_t\tsta_mac\t")
            ]
            self.assertEqual(len(remaining), len(lines) - 1)
            path.write_text("".join(remaining), encoding="utf-8")
            result = self.run_generator("--check", "--out", path)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("inventory fixture is stale", result.stderr)

    def test_check_compares_fixture_bytes_including_line_endings(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "inventory.tsv"
            original = (DATA / "dm_inventory.tsv").read_bytes()
            path.write_bytes(original.replace(b"\n", b"\r\n"))
            result = self.run_generator("--check", "--out", path)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("inventory fixture is stale", result.stderr)

    def test_validation_precedes_write(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "output.tsv"
            path.write_text("do not replace\n", encoding="utf-8")
            classification = Path(tmp) / "classification.tsv"
            original = (DATA / "dm_type_classification.tsv").read_text(encoding="utf-8")
            classification.write_text(
                original.replace("\tdual-use\t", "\tinvalid\t", 1), encoding="utf-8"
            )
            result = self.run_generator(
                "--out", path, "--classification", classification
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("unknown category", result.stderr)
            self.assertEqual(path.read_text(encoding="utf-8"), "do not replace\n")

    def test_failed_atomic_replace_preserves_existing_fixture(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "output.tsv"
            path.write_text("original\n", encoding="utf-8")
            with mock.patch.object(
                gen.os, "replace", side_effect=OSError("replace failed")
            ), self.assertRaises(gen.InputError):
                gen.atomic_write(path, "replacement\n")
            self.assertEqual(path.read_text(encoding="utf-8"), "original\n")
            self.assertEqual(list(Path(tmp).iterdir()), [path])

    def test_interrupted_atomic_replace_removes_staging_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "output.tsv"
            path.write_text("original\n", encoding="utf-8")
            with mock.patch.object(gen.os, "replace", side_effect=KeyboardInterrupt):
                with self.assertRaises(KeyboardInterrupt):
                    gen.atomic_write(path, "replacement\n")
            self.assertEqual(path.read_text(encoding="utf-8"), "original\n")
            self.assertEqual(list(Path(tmp).iterdir()), [path])

    def test_invalid_utf8_manifest_is_content_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "manifest.tsv"
            path.write_bytes(b"\xff\n")
            result = self.run_generator("--check", "--manifest", path)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("UTF-8", result.stderr)

    def test_invalid_utf8_classification_is_content_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "classification.tsv"
            path.write_bytes(b"\xff\n")
            result = self.run_generator("--check", "--classification", path)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("UTF-8", result.stderr)

    def test_missing_git_checkout_is_io_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = self.run_generator("--check", "--upstream", Path(tmp) / "absent")
            self.assertEqual(result.returncode, 2, result.stderr)

    def test_real_policy_and_hal_mappings_are_in_fixture(self):
        with (DATA / "dm_inventory.tsv").open(encoding="utf-8", newline="") as file:
            rows = list(
                csv.DictReader(
                    (line for line in file if not line.startswith("#")), delimiter="\t"
                )
            )
        index = {(row["type"], row["field"]): row for row in rows}
        self.assertEqual(
            index[("em_policy_t", "client_filters")]["c_type"],
            "em_client_filters_cfg_t",
        )
        self.assertEqual(
            index[("em_client_filters_cfg_t", "sta_mac")]["c_type"], "mac_addr_t"
        )
        self.assertEqual(index[("mac_addr_t", "<alias>")]["bound_value"], "6")
        self.assertEqual(index[("ssid_t", "<alias>")]["bound_value"], "32")
        self.assertEqual(
            index[("wifi_BeaconReport_t", "wideBandwidthChannel")]["c_type"],
            "wifi_WideBWChannel_t",
        )

    @unittest.skipUnless(
        shutil.which("c++") and shutil.which("readelf"),
        "C++ compiler or readelf is unavailable",
    )
    def test_pinned_declarations_bounds_and_enums_compile(self):
        # Compile pinned declarations, then compare every emitted field name,
        # order and bit width with the compiler's independent DWARF field list.
        # Static assertions also compare each emitted type and array extent.
        # This isolated declaration check is not a full upstream header build.
        parent = REPO.parent
        keys = [
            ("OneWifi", "include/wifi_base.h"),
            ("rdkb-halif-wifi", "include/wifi_hal_generic.h"),
            ("rdkb-halif-wifi", "include/wifi_hal_extender.h"),
            ("rdkb-halif-wifi", "include/wifi_hal_ap.h"),
        ]
        sources = {
            (repo, path): gen.Source(
                gen.read_git(parent / repo, gen.PINS[repo][0], path), path
            )
            for repo, path in keys
        }
        base = gen.Source(
            gen.read_git(parent / "unified-wifi-mesh", gen.UPSTREAM, "inc/em_base.h"),
            "inc/em_base.h",
        )
        manifest = gen.validate_manifest(
            gen.read_tsv(DATA / "dm_external_manifest.tsv", gen.MANIFEST_COLUMNS),
            sources,
        )
        walker = gen.Walker(base, sources, manifest)
        for root in gen.ROOTS:
            walker.walk(root)

        code = [
            "#include <cstddef>",
            "#include <cstdint>",
            "#include <ctime>",
            "#include <type_traits>",
        ]
        for name in sorted(walker.constants):
            if name in base.constants:
                code.append(f"#define {name} {base.constants[name][0][0]}")
            elif name in manifest and manifest[name]["kind"] == "constant":
                code.append(f"#define {name} {manifest[name]['definition']}")
        for name, row in sorted(manifest.items()):
            if row["kind"] == "type-macro":
                code.append(f"#define {name} {row['definition']}")

        declarations = {
            name: decl for name, decl in walker.reached.items() if decl is not None
        }
        # All reached enums at the pin have literal or implicit values and
        # can precede the typedefs whose array bounds use their enumerators.
        emitted = set()
        for name, decl in declarations.items():
            if decl.kind == "enum":
                code.append(decl.definition)
                emitted.add(name)
        dependencies = {name: set() for name in declarations}
        for owner, _, typ, _, _, _, _, _, _ in walker.rows:
            dependency = gen.canonical_type(typ.replace("*", " "))
            if (
                owner in dependencies
                and dependency in declarations
                and dependency != owner
            ):
                dependencies[owner].add(dependency)
        while len(emitted) < len(declarations):
            ready = sorted(
                name
                for name in declarations
                if name not in emitted and dependencies[name] <= emitted
            )
            self.assertTrue(ready, "declaration dependency cycle")
            for name in ready:
                code.append(declarations[name].definition)
                emitted.add(name)

        code += [
            "static_assert(sizeof(void*) == 8 && sizeof(long) == 8);",
            "static_assert(sizeof(bool) == 1 && sizeof(uint8_t) == 1 && sizeof(uint32_t) == 4);",
            "static_assert(sizeof(time_t) == 8 && sizeof(size_t) == 8);",
            "static_assert(std::is_same<uint8_t, unsigned char>::value);",
            "static_assert(std::is_same<uint32_t, unsigned int>::value);",
            "static_assert(std::is_same<time_t, signed long>::value);",
            "static_assert(std::is_same<size_t, unsigned long>::value);",
            "static_assert(sizeof(ssid_t) == 32 && sizeof(mac_addr_t) == 6 && sizeof(mac_address_t) == 6);",
        ]
        for owner, field, typ, _, value, _, _, enum_value, _ in walker.rows:
            if typ == "enumerator":
                code.append(f"static_assert({field} == {enum_value});")
            elif field != "<mapping>":
                expression = (
                    owner
                    if field == "<alias>"
                    else f"decltype((({owner}*)nullptr)->{field})"
                )
                dimensions = [] if value == "-" else value.split(";")
                expected_type = typ + "".join(f"[{bound}]" for bound in dimensions)
                code.append(
                    f"static_assert(std::is_same<{expression}, {expected_type}>::value, "
                    f'"field type {owner}.{field}");'
                )
                for dimension, expected in enumerate(dimensions):
                    code.append(
                        f"static_assert(std::extent<{expression}, {dimension}>::value == {expected});"
                    )
        with tempfile.TemporaryDirectory() as tmp:
            obj = Path(tmp) / "declarations.o"
            result = subprocess.run(
                [
                    "c++",
                    "-std=c++17",
                    "-g",
                    "-fno-eliminate-unused-debug-types",
                    "-x",
                    "c++",
                    "-c",
                    "-o",
                    str(obj),
                    "-",
                ],
                input="\n".join(code) + "\n",
                text=True,
                capture_output=True,
                check=False,
                env={**os.environ, "CCACHE_DISABLE": "1"},
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            members = compiler_members(obj)
        assert_compiler_members(self, walker, members)

        # Prove the independent field-list comparison rejects the exact scalar
        # omission that the former compiler-only assertions failed to detect.
        saved = walker.rows
        try:
            walker.rows = [
                row
                for row in saved
                if row[:2] != ("em_metric_cltn_interval_t", "metric_cltn_interval")
            ]
            self.assertEqual(len(walker.rows), len(saved) - 1)
            with self.assertRaisesRegex(
                AssertionError,
                "compiler field list differs for em_metric_cltn_interval_t",
            ):
                assert_compiler_members(self, walker, members)
        finally:
            walker.rows = saved


if __name__ == "__main__":
    unittest.main()
