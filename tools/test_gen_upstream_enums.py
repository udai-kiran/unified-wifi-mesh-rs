# SPDX-License-Identifier: Apache-2.0
import contextlib
import io
import importlib.machinery
import importlib.util
import pathlib
import tempfile
import unittest

_P = pathlib.Path(__file__).resolve().parent / "gen-upstream-enums"
_loader = importlib.machinery.SourceFileLoader("gen_upstream_enums", str(_P))
_spec = importlib.util.spec_from_loader("gen_upstream_enums", _loader)
gen = importlib.util.module_from_spec(_spec)
_loader.exec_module(gen)

_CLS_HEADER = (
    "upstream_type\tupstream_identifier\tclass\trust_name\tspec_reference\treason\n"
)


def _suppress():
    """Suppress stderr so SystemExit tests don't pollute -v output."""
    return contextlib.redirect_stderr(io.StringIO())


class TestExtractEnums(unittest.TestCase):

    def test_extract_one_enum_with_comments(self):
        """Test 1: synthetic header with one enum, block+line comments with commas."""
        # The \n is a real newline in this regular string.
        header = "typedef enum { a = 1, /* x, y */ b, // c, d\n c, } t_t;"
        with _suppress():
            result = gen.extract_enums(header, enums=[("t_t", 3, 2)])
        self.assertEqual(len(result), 1)
        name, _body, idents = result[0]
        self.assertEqual(name, "t_t")
        self.assertEqual(idents, ["a", "b", "c"])

    def test_extract_ignores_other_enum(self):
        """Test 2: earlier enum does not bleed into t_t (cross-block bug regression)."""
        header = "typedef enum { z } other_t; typedef enum { a, b } t_t;"
        with _suppress():
            result = gen.extract_enums(header, enums=[("t_t", 2, 2)])
        self.assertEqual(len(result), 1)
        _name, _body, idents = result[0]
        self.assertEqual(idents, ["a", "b"])

    def test_extract_count_mismatch_exit1(self):
        """Test 3: expected count mismatch → exit 1."""
        header = "typedef enum { a, b, c } t_t;"
        with self.assertRaises(SystemExit) as cm:
            with _suppress():
                gen.extract_enums(header, enums=[("t_t", 5, 2)])
        self.assertEqual(cm.exception.code, 1)

    def test_extract_preprocessor_line_exit1(self):
        """Test 4: #if line inside enum body → invalid identifier → exit 1."""
        header = "typedef enum { a,\n#if FOO\nb } t_t;"
        with self.assertRaises(SystemExit) as cm:
            with _suppress():
                gen.extract_enums(header, enums=[("t_t", 2, 2)])
        self.assertEqual(cm.exception.code, 1)

    def test_extract_defined_twice_exit1(self):
        """Test 5: target enum defined twice → more than 1 match → exit 1."""
        header = "typedef enum { a } t_t; typedef enum { b } t_t;"
        with self.assertRaises(SystemExit) as cm:
            with _suppress():
                gen.extract_enums(header, enums=[("t_t", 1, 2)])
        self.assertEqual(cm.exception.code, 1)


class TestParseOutput(unittest.TestCase):

    def _enum2(self):
        """Shared fixtures: 2-identifier enum, width 2."""
        enums = [("my_t", 2, 2)]
        enum_data = [("my_t", " a, b", ["a", "b"])]
        return enums, enum_data

    def test_parse_correct_output(self):
        """Test 6: complete correct output → rows with hex values formatted to width."""
        enums = [("my_t", 2, 2)]
        enum_data = [("my_t", " a, b", ["a", "b"])]
        output = "my_t\ta\t1\nmy_t\tb\t255\n"
        rows = gen.parse_output(output, enum_data, enums=enums)
        self.assertEqual(rows, [("my_t", "a", "0x01"), ("my_t", "b", "0xFF")])

    def test_parse_empty_output_exit2(self):
        """Test 7: empty output when identifiers are expected → exit 2."""
        enums, enum_data = self._enum2()
        with self.assertRaises(SystemExit) as cm:
            with _suppress():
                gen.parse_output("", enum_data, enums=enums)
        self.assertEqual(cm.exception.code, 2)

    def test_parse_one_row_missing_exit2(self):
        """Test 8: one row missing from output → exit 2."""
        enums, enum_data = self._enum2()
        output = "my_t\ta\t0\n"  # only the first of two expected rows
        with self.assertRaises(SystemExit) as cm:
            with _suppress():
                gen.parse_output(output, enum_data, enums=enums)
        self.assertEqual(cm.exception.code, 2)

    def test_parse_rows_reordered_exit2(self):
        """Test 9: two rows in wrong order → completeness check → exit 2."""
        enums, enum_data = self._enum2()
        output = "my_t\tb\t1\nmy_t\ta\t0\n"  # b before a, expected a then b
        with self.assertRaises(SystemExit) as cm:
            with _suppress():
                gen.parse_output(output, enum_data, enums=enums)
        self.assertEqual(cm.exception.code, 2)

    def test_parse_extra_unknown_row_exit2(self):
        """Test 10: extra row with unknown c_name → exit 2."""
        enums, enum_data = self._enum2()
        output = "my_t\ta\t0\nmy_t\tb\t1\nunknown_t\tc\t2\n"
        with self.assertRaises(SystemExit) as cm:
            with _suppress():
                gen.parse_output(output, enum_data, enums=enums)
        self.assertEqual(cm.exception.code, 2)


class TestValidateClassification(unittest.TestCase):

    def test_valid_classification_returns_empty(self):
        """Test 11: valid 2-row classification for 2 fixture rows → empty list."""
        rows = [("t_t", "a", "0x00"), ("t_t", "b", "0x01")]
        content = (
            _CLS_HEADER
            + "t_t\ta\tspec\tA\tTable 1 §1\t-\n"
            + "t_t\tb\tspec\tB\tTable 1 §2\t-\n"
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            cls_path = pathlib.Path(tmpdir) / "classification.tsv"
            cls_path.write_text(content, encoding="utf-8")
            errors = gen.validate_classification(cls_path, rows, False)
        self.assertEqual(errors, [])

    def test_duplicate_rows_names_lines_3_and_4(self):
        """Test 12: blank at line 2 then duplicate rows at lines 3 and 4."""
        rows = [("t_t", "a", "0x00")]
        content = (
            _CLS_HEADER
            + "\n"                                       # blank: line 2
            + "t_t\ta\tspec\tA\tTable 1 §1\t-\n"        # line 3
            + "t_t\ta\tspec\tB\tTable 1 §1\t-\n"        # line 4 (duplicate pair, different rust_name)
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            cls_path = pathlib.Path(tmpdir) / "classification.tsv"
            cls_path.write_text(content, encoding="utf-8")
            errors = gen.validate_classification(cls_path, rows, False)
        pair_msgs = [e for e in errors if "duplicate (upstream_type, upstream_identifier) pair" in e]
        self.assertEqual(len(pair_msgs), 1, f"expected one duplicate-pair error; got: {errors}")
        self.assertIn("at lines 3 and 4", pair_msgs[0])
        self.assertFalse(
            any("duplicate (upstream_type, rust_name) pair" in e for e in errors),
            f"rust_names differ, so no duplicate-rust_name error expected; got: {errors}",
        )

    def test_duplicate_rust_name_names_lines_3_and_4(self):
        """Test 16: blank at line 2 then two different identifiers sharing a rust_name at lines 3 and 4."""
        rows = [("t_t", "a", "0x00"), ("t_t", "b", "0x01")]
        content = (
            _CLS_HEADER
            + "\n"                                       # blank: line 2
            + "t_t\ta\tspec\tA\tTable 1 §1\t-\n"        # line 3
            + "t_t\tb\tspec\tA\tTable 1 §1\t-\n"        # line 4 (same rust_name)
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            cls_path = pathlib.Path(tmpdir) / "classification.tsv"
            cls_path.write_text(content, encoding="utf-8")
            errors = gen.validate_classification(cls_path, rows, False)
        name_msgs = [e for e in errors if "duplicate (upstream_type, rust_name) pair" in e]
        self.assertEqual(len(name_msgs), 1, f"expected one duplicate-rust_name error; got: {errors}")
        self.assertIn("at lines 3 and 4", name_msgs[0])
        self.assertFalse(
            any("duplicate (upstream_type, upstream_identifier) pair" in e for e in errors),
            f"identifiers differ, so no duplicate-pair error expected; got: {errors}",
        )

    def test_whitespace_spec_reference_error(self):
        """Test 13: whitespace-only spec_reference on a spec row."""
        rows = [("t_t", "a", "0x00")]
        content = (
            _CLS_HEADER
            + "t_t\ta\tspec\tA\t  \t-\n"  # spec_reference is two spaces
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            cls_path = pathlib.Path(tmpdir) / "classification.tsv"
            cls_path.write_text(content, encoding="utf-8")
            errors = gen.validate_classification(cls_path, rows, False)
        self.assertTrue(
            any("spec_reference" in e for e in errors),
            f"expected 'spec_reference' in some error; got: {errors}",
        )
        self.assertFalse(
            any("missing entry" in e for e in errors),
            f"unexpected 'missing entry' in errors: {errors}",
        )
        self.assertTrue(
            any(
                "coverage/order check skipped because of the errors above" in e
                for e in errors
            ),
            f"expected skip-sentinel message in errors; got: {errors}",
        )

    def test_spec_row_rust_name_dash_error(self):
        """Test 14: spec row with rust_name '-' → error mentioning rust_name."""
        rows = [("t_t", "a", "0x00")]
        content = (
            _CLS_HEADER
            + "t_t\ta\tspec\t-\tTable 1 §1\t-\n"
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            cls_path = pathlib.Path(tmpdir) / "classification.tsv"
            cls_path.write_text(content, encoding="utf-8")
            errors = gen.validate_classification(cls_path, rows, False)
        self.assertTrue(
            any("rust_name" in e for e in errors),
            f"expected 'rust_name' in some error; got: {errors}",
        )

    def test_missing_file_returns_none(self):
        """Test 15: missing classification file → returns None."""
        with tempfile.TemporaryDirectory() as tmpdir:
            cls_path = pathlib.Path(tmpdir) / "nonexistent.tsv"
            result = gen.validate_classification(cls_path, [], False)
        self.assertIsNone(result)


if __name__ == "__main__":
    unittest.main()
