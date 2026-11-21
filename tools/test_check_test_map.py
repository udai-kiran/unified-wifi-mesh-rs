"""Tests for tools/check-test-map."""
# SPDX-License-Identifier: Apache-2.0

import contextlib
import importlib.machinery
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
LOADER = importlib.machinery.SourceFileLoader("check_test_map", str(ROOT / "tools/check-test-map"))
SPEC = importlib.util.spec_from_loader(LOADER.name, LOADER)
tool = importlib.util.module_from_spec(SPEC)
LOADER.exec_module(tool)

HEADER = "# a comment\ntest\tline\tdisposition\ttag\tnotes\n"
EXISTING = {"a::tests::one", "a::tests::two"}


def map_text(*rows):
    return HEADER + "".join("\t".join(row) + "\n" for row in rows)


class CheckTestMapTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)

    def write(self, name, text):
        path = self.dir / name
        path.write_text(text)
        return path

    def check(self, map_text_, source_text=None, existing=EXISTING):
        map_path = self.write("map.tsv", map_text_)
        source_path = None if source_text is None else self.write("readers.rs", source_text)
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            problems = tool.check_crate("crate", map_path, source_path, existing)
        return problems, out.getvalue()

    def test_all_named_tests_exist(self):
        rows = map_text(
            ("t1", "1", "rust:a::tests::one", "parity", ""),
            ("t2", "2", "skip:not ported", "parity", ""),
        )
        problems, out = self.check(rows)
        self.assertEqual(problems, 0)
        self.assertIn("crate: 1 distinct Rust tests named, 0 missing", out)

    def test_a_missing_rust_test_is_reported(self):
        rows = map_text(
            ("t1", "1", "rust:a::tests::one", "parity", ""),
            ("t2", "2", "rust:a::tests::gone", "parity", ""),
        )
        problems, out = self.check(rows)
        self.assertEqual(problems, 1)
        self.assertIn("missing: a::tests::gone (upstream tests: t2)", out)

    def test_a_tag_in_the_source_without_a_row_is_reported(self):
        rows = map_text(("t1", "1", "rust:a::tests::one", "intentional-difference:kept", ""))
        source = "// intentional-difference:kept\n// intentional-difference:forgotten\n"
        problems, out = self.check(rows, source)
        self.assertEqual(problems, 1)
        self.assertIn("tag without a map row: forgotten", out)

    def test_a_row_with_a_tag_absent_from_the_source_is_reported(self):
        rows = map_text(
            ("t1", "1", "rust:a::tests::one", "intentional-difference:kept", ""),
            ("t2", "2", "rust:a::tests::two", "intentional-difference:invented", ""),
        )
        problems, out = self.check(rows, "// intentional-difference:kept\n")
        self.assertEqual(problems, 1)
        self.assertIn("map row with a tag absent from readers.rs: invented", out)

    def test_matching_tags_pass_and_two_rows_may_share_a_tag(self):
        rows = map_text(
            ("t1", "1", "rust:a::tests::one", "intentional-difference:kept", ""),
            ("t2", "2", "rust:a::tests::two", "intentional-difference:kept", ""),
        )
        problems, _ = self.check(rows, "// intentional-difference:kept\n// intentional-difference:kept\n")
        self.assertEqual(problems, 0)

    def test_without_a_tag_source_tags_are_not_compared(self):
        rows = map_text(("t1", "1", "rust:a::tests::one", "intentional-difference:any", ""))
        problems, _ = self.check(rows)
        self.assertEqual(problems, 0)

    def test_main_checks_each_crate_with_its_own_map_and_tag_source(self):
        dm_map = self.write("dm.tsv", map_text(("t1", "1", "rust:a::tests::one", "parity", "")))
        msg_map = self.write("msg.tsv", map_text(("t1", "1", "rust:a::tests::one", "parity", "")))
        source = self.write("readers.rs", "// intentional-difference:unmapped\n")
        crates = [
            {"name": "em-dm", "map": dm_map, "tag_source": None},
            {"name": "em-msg", "map": msg_map, "tag_source": source},
        ]
        listed = []

        def fake_listed(crate):
            listed.append(crate)
            return EXISTING

        out = io.StringIO()
        with (
            mock.patch.object(tool, "CRATES", crates),
            mock.patch.object(tool, "listed_tests", fake_listed),
            contextlib.redirect_stdout(out),
        ):
            code = tool.main()
        self.assertEqual(listed, ["em-dm", "em-msg"])
        self.assertEqual(code, 1)
        self.assertIn("em-dm: 1 distinct Rust tests named, 0 missing", out.getvalue())
        self.assertIn("tag without a map row: unmapped", out.getvalue())

    def test_em_dm_has_no_tag_source(self):
        by_name = {crate["name"]: crate for crate in tool.CRATES}
        self.assertIsNone(by_name["em-dm"]["tag_source"])
        self.assertEqual(by_name["em-msg"]["tag_source"], ROOT / "crates/em-msg/src/readers.rs")

    def test_the_real_em_msg_map_agrees_with_readers_rs_tags(self):
        by_name = {crate["name"]: crate for crate in tool.CRATES}
        msg = by_name["em-msg"]
        self.assertEqual(tool.mapped_tags(msg["map"]), tool.source_tags(msg["tag_source"]))

READER_NAMES = list(tool.EXPECTED_READER_ROWS)
SKIPPED = tool.NOT_PORTED
TEST_SOURCE = """mod tests {
    #[test]
    fn one() {
        // intentional-difference:kept
        assert!(true);
    }

    #[test]
    fn two() {
        assert!(true);
    }
}
"""


def reader_row(name):
    disposition = tool.EXPECTED_READER_ROWS[name] or "rust:readers::tests::one"
    return (name, "1", "reader", "PORT", "-", disposition, "parity", "")


def msg_map_text(*extra_rows, readers=None):
    header = "test\tline\tscope\ttriage_verdict\ttriage_method\tdisposition\ttag\tnotes\n"
    names = READER_NAMES if readers is None else readers
    rows = [reader_row(name) for name in names] + list(extra_rows)
    return "# comment\n" + header + "".join("\t".join(row) + "\n" for row in rows)


def tag_row(test, disposition, tag="intentional-difference:kept"):
    return (test, "1", "tag", "PORT", "-", disposition, tag, "")


class MapShapeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)

    def problems(self, map_text_, source_text=TEST_SOURCE):
        map_path = self.dir / "map.tsv"
        map_path.write_text(map_text_)
        source_path = self.dir / "readers.rs"
        source_path.write_text(source_text)
        return tool.map_shape_problems(map_path, source_path)

    def test_a_well_formed_map_has_no_problems(self):
        extra = tag_row("kept", "rust:readers::tests::one")
        self.assertEqual(self.problems(msg_map_text(extra)), [])

    def test_a_disposition_that_is_neither_rust_nor_skip_is_reported(self):
        extra = ("odd", "1", "tag", "PORT", "-", "todo", "parity", "")
        messages = self.problems(msg_map_text(extra))
        self.assertEqual(len(messages), 1)
        self.assertIn("neither rust:<path> nor skip:<reason>: odd (todo)", messages[0])

    def test_an_empty_rust_or_skip_disposition_is_reported(self):
        extra = (
            ("empty-rust", "1", "tag", "PORT", "-", "rust:", "parity", ""),
            ("empty-skip", "1", "tag", "SKIP", "-", "skip:", "parity", ""),
        )
        self.assertEqual(len(self.problems(msg_map_text(*extra))), 2)

    def test_each_missing_upstream_reader_row_is_reported(self):
        for missing in READER_NAMES:
            with self.subTest(missing=missing):
                names = [name for name in READER_NAMES if name != missing]
                messages = self.problems(msg_map_text(readers=names))
                self.assertEqual(messages, [f"upstream reader row missing: {missing}"])

    def test_a_not_ported_row_that_became_a_rust_row_is_reported(self):
        names = [name for name in READER_NAMES if name != "get_client_mac_info"]
        extra = ("get_client_mac_info", "1", "reader", "PORT", "-", "rust:readers::tests::one", "parity", "")
        messages = self.problems(msg_map_text(extra, readers=names))
        self.assertEqual(len(messages), 1)
        self.assertIn("reader row get_client_mac_info must be", messages[0])

    def test_a_tagged_row_that_is_a_skip_is_reported(self):
        extra = tag_row("kept", "skip:not ported")
        messages = self.problems(msg_map_text(extra))
        self.assertEqual(messages, ["tagged row is not a rust: reference: kept (skip:not ported)"])

    def test_a_tag_outside_the_body_of_the_named_test_is_reported(self):
        extra = tag_row("kept", "rust:readers::tests::two")
        messages = self.problems(msg_map_text(extra))
        self.assertEqual(len(messages), 1)
        self.assertIn("tag intentional-difference:kept is not inside the body of readers::tests::two", messages[0])

    def test_a_tag_naming_another_id_than_the_test_body_is_reported(self):
        extra = tag_row("other", "rust:readers::tests::one", "intentional-difference:other")
        messages = self.problems(msg_map_text(extra))
        self.assertEqual(len(messages), 1)
        self.assertIn("is not inside the body of readers::tests::one", messages[0])

    def test_a_ported_reader_row_that_became_a_skip_is_reported(self):
        for name in READER_NAMES:
            if tool.EXPECTED_READER_ROWS[name] is not None:
                continue
            with self.subTest(name=name):
                names = [other for other in READER_NAMES if other != name]
                extra = (name, "1", "reader", "PORT", "-", "skip:later", "parity", "")
                messages = self.problems(msg_map_text(extra, readers=names))
                self.assertEqual(messages, [f"ported reader row {name} must be rust:, not 'skip:later'"])

    def test_a_duplicate_reader_row_is_reported(self):
        extra = ("get_client_mac_info", "1", "reader", "PORT", "-", "rust:readers::tests::one", "parity", "")
        rows = [extra] + [reader_row(name) for name in READER_NAMES]
        header = "test\tline\tscope\ttriage_verdict\ttriage_method\tdisposition\ttag\tnotes\n"
        text = "# comment\n" + header + "".join("\t".join(row) + "\n" for row in rows)
        self.assertEqual(self.problems(text), ["reader row appears 2 times: get_client_mac_info"])

    def test_a_tag_that_is_only_a_prefix_of_the_body_tag_is_reported(self):
        extra = tag_row("kep", "rust:readers::tests::one", "intentional-difference:kep")
        messages = self.problems(msg_map_text(extra))
        self.assertEqual(len(messages), 1)
        self.assertIn("is not inside the body of readers::tests::one", messages[0])

    def pair_problems(self, map_text_, source_text):
        map_path = self.dir / "map.tsv"
        map_path.write_text(map_text_)
        source_path = self.dir / "readers.rs"
        source_path.write_text(source_text)
        return tool.tag_pair_problems(map_path, source_path)

    def test_each_test_carrying_a_shared_tag_needs_its_own_row(self):
        source = TEST_SOURCE.replace(
            "fn two() {\n", "fn two() {\n        // intentional-difference:kept\n"
        )
        one = tag_row("kept", "rust:readers::tests::one")
        two = tag_row("kept (two)", "rust:readers::tests::two")
        self.assertEqual(self.pair_problems(msg_map_text(one, two), source), [])
        self.assertEqual(
            self.pair_problems(msg_map_text(one), source),
            ["tag kept in test two has no map row"],
        )

    def test_a_tagged_row_naming_a_test_absent_from_readers_rs_is_reported(self):
        extra = tag_row("kept", "rust:readers::tests::gone")
        messages = self.problems(msg_map_text(extra))
        self.assertEqual(len(messages), 1)
        self.assertIn("names a test not found in readers.rs: readers::tests::gone", messages[0])

    def test_main_runs_the_shape_rules_only_for_a_crate_with_shape(self):
        bad_map = self.dir / "bad.tsv"
        # The tag row keeps the tag comparison quiet, so only the shape rules can fail.
        bad_map.write_text(
            msg_map_text(tag_row("kept", "rust:readers::tests::one"), readers=READER_NAMES[1:])
        )
        source = self.dir / "readers.rs"
        source.write_text(TEST_SOURCE)
        for shape, expected_code in [(True, 1), (False, 0)]:
            with self.subTest(shape=shape):
                crates = [{"name": "em-msg", "map": bad_map, "tag_source": source, "shape": shape}]
                with (
                    mock.patch.object(tool, "CRATES", crates),
                    mock.patch.object(tool, "listed_tests", lambda crate: {"readers::tests::one"}),
                    contextlib.redirect_stdout(io.StringIO()),
                ):
                    self.assertEqual(tool.main(), expected_code)

    def test_the_real_em_msg_map_has_no_shape_problems(self):
        by_name = {crate["name"]: crate for crate in tool.CRATES}
        msg = by_name["em-msg"]
        self.assertEqual(tool.map_shape_problems(msg["map"], msg["tag_source"]), [])

    def test_em_dm_has_no_shape_rules(self):
        by_name = {crate["name"]: crate for crate in tool.CRATES}
        self.assertFalse(by_name["em-dm"]["shape"])
        self.assertTrue(by_name["em-msg"]["shape"])


if __name__ == "__main__":
    unittest.main()
