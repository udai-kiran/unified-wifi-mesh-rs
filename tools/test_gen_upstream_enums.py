# SPDX-License-Identifier: Apache-2.0
import contextlib
import io
import importlib.machinery
import importlib.util
import pathlib
import sys
import tempfile
import unittest
from unittest import mock

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


def _exit_of(func, *args, **kwargs):
    """Call func, which must raise SystemExit; return (exit code, stderr text)."""
    stderr = io.StringIO()
    with contextlib.redirect_stderr(stderr):
        try:
            func(*args, **kwargs)
        except SystemExit as exc:
            return exc.code, stderr.getvalue()
    raise AssertionError("expected SystemExit")


def _synthetic_header():
    """A header declaring every enum in gen.ENUMS with the expected count."""
    blocks = []
    for name, count, _width in gen.ENUMS:
        body = ",\n".join(f"    {name.upper()}_{i} = {i}" for i in range(count))
        blocks.append(f"typedef enum {{\n{body}\n}} {name};")
    return "\n".join(blocks) + "\n"


def _run_main(*cli_args):
    """Run gen.main() on a synthetic header; return (exit code, stdout, stderr)."""
    stdout = io.StringIO()
    stderr = io.StringIO()
    argv = ["gen-upstream-enums", "--upstream", "unused"] + [str(a) for a in cli_args]
    code = 0
    with mock.patch.object(sys, "argv", argv), \
            mock.patch.object(gen, "read_header", return_value=_synthetic_header()), \
            contextlib.redirect_stdout(stdout), \
            contextlib.redirect_stderr(stderr):
        try:
            gen.main()
        except SystemExit as exc:
            code = exc.code
    return code, stdout.getvalue(), stderr.getvalue()


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
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 5, 2)])
        self.assertEqual(code, 1)
        self.assertIn("expected 5 enumerators, found 3", err)

    def test_extract_preprocessor_line_exit1(self):
        """Test 4: #if line inside enum body → exit 1 naming the directive."""
        header = "typedef enum { a,\n#if FOO\nb } t_t;"
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 2, 2)])
        self.assertEqual(code, 1)
        self.assertIn("preprocessor directive", err)
        self.assertIn("#if FOO", err)

    def test_extract_preprocessor_in_initializer_exit1(self):
        """A #if between '=' and the value is rejected, not skipped over."""
        header = "typedef enum { a = \n#if FOO\n1\n#else\n2\n#endif\n, b } t_t;"
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 2, 2)])
        self.assertEqual(code, 1)
        self.assertIn("preprocessor directive", err)

    def test_extract_hash_inside_comment_is_allowed(self):
        """A '#' at the start of a comment line is not a directive."""
        header = "typedef enum { a, /*\n#if not real\n*/ b } t_t;"
        result = gen.extract_enums(header, enums=[("t_t", 2, 2)])
        self.assertEqual(result[0][2], ["a", "b"])

    def test_extract_defined_twice_exit1(self):
        """Test 5: target enum defined twice → more than 1 match → exit 1."""
        header = "typedef enum { a } t_t; typedef enum { b } t_t;"
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 1, 2)])
        self.assertEqual(code, 1)
        self.assertIn("expected exactly 1 match, found 2", err)

    def test_extract_digraph_directive_in_initializer_exit1(self):
        """R1-05: a %:ifdef digraph directive in an initializer is rejected."""
        header = "typedef enum { a = \n%:ifdef EM_BASE_H\n10\n%:else\n20\n%:endif\n, b } t_t;"
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 2, 2)])
        self.assertEqual(code, 1)
        self.assertIn("digraph", err)

    def test_extract_splice_outside_define_exit1(self):
        """R2-04: backslash-newline outside a #define is rejected, naming the line."""
        header = "typedef enum { a = \n#\\\nif FOO\n1\n#endif\n, b } t_t;"
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 2, 2)])
        self.assertEqual(code, 1)
        self.assertIn("line 2", err)
        self.assertIn("backslash-newline", err)

    def test_extract_splice_in_identifier_exit1(self):
        """R2-04: a splice inside an enum body is rejected, not joined."""
        header = "typedef enum { al\\\npha, beta } t_t;"
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 2, 2)])
        self.assertEqual(code, 1)
        self.assertIn("line 1", err)

    def test_extract_splice_forming_trigraph_exit1(self):
        """R2-04: the reviewer's sizeof("?<splice>?-") reproduction exits 1."""
        header = 'typedef enum { a = sizeof("?\\\n?-") } t_t;'
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 1, 2)])
        self.assertEqual(code, 1)
        self.assertIn("line 1", err)

    def test_extract_continued_define_elsewhere_accepted(self):
        """R2-04: a continued #define, and its continuation lines, may splice."""
        header = (
            "#define TWO (1 | \\\n   2 | \\\n   4)\n"
            "typedef enum { a, b } t_t;\n"
        )
        result = gen.extract_enums(header, enums=[("t_t", 2, 2)])
        self.assertEqual(result[0][2], ["a", "b"])

    def test_extract_splice_after_define_ends_exit1(self):
        """R2-04: the define ends at its last continuation line."""
        header = "#define X 1 \\\n  + 2\ntypedef enum { a, \\\nb } t_t;\n"
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 2, 2)])
        self.assertEqual(code, 1)
        self.assertIn("line 3", err)

    def test_extract_declaration_in_spliced_comment_not_matched(self):
        """R2-01: reviewer's repro: a comment built by splices hides a declaration.

        The splices sit in a #define so R2-04 allows them. The only live
        declaration is tagged, so the untagged form has no match.
        """
        header = (
            "#define X /\\\n/ type\\\ndef enum { a = 7 } t_t;\n"
            "typedef enum actual_tag { a = 9 } t_t;\n"
        )
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 1, 2)])
        self.assertEqual(code, 1)
        self.assertIn("expected exactly 1 match, found 0", err)

    def test_extract_declaration_in_line_comment_decoy_ignored(self):
        """R2-01: a commented-out declaration beside the real one is ignored."""
        header = (
            "// typedef enum { a = 7 } t_t;\n"
            "typedef enum { a = 9 } t_t;\n"
        )
        result = gen.extract_enums(header, enums=[("t_t", 1, 2)])
        self.assertEqual(result[0][2], ["a"])
        self.assertIn("9", result[0][1])
        self.assertNotIn("7", result[0][1])

    def test_extract_declaration_in_string_or_block_comment_ignored(self):
        """R2-01: declarations in a string or block comment are not matched."""
        header = (
            'const char *s = "typedef enum { a = 7 } t_t;";\n'
            "/* typedef enum { a = 8 } t_t; */\n"
            "typedef enum { a = 9 } t_t;\n"
        )
        result = gen.extract_enums(header, enums=[("t_t", 1, 2)])
        self.assertIn("9", result[0][1])
        self.assertNotIn("7", result[0][1])
        self.assertNotIn("8", result[0][1])

    def test_extract_unsupported_lexical_constructs_exit1(self):
        """Raw/prefixed literals, digit separators, digraphs, trigraphs exit 1."""
        cases = {
            "raw string": 'a = sizeof(R"x(1)x"), b',
            "prefixed char": "a = L'x', b",
            "prefixed string": 'a = sizeof(u8"x"), b',
            "digit separator": "a = 1'000, b",
            "digraph": "a = 1 %: 2, b",
        }
        for label, body in cases.items():
            with self.subTest(label):
                header = "typedef enum { " + body + " } t_t;"
                code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 2, 2)])
                self.assertEqual(code, 1)
                self.assertIn("unsupported", err)

    def test_extract_constructs_inside_comments_and_plain_literals_allowed(self):
        """The rejections apply to code only, not to comments or plain literals."""
        header = (
            'typedef enum { a = sizeof("%: R\\"( L\'"), /* <% 1\'000 */ b } t_t;'
        )
        result = gen.extract_enums(header, enums=[("t_t", 2, 2)])
        self.assertEqual(result[0][2], ["a", "b"])


class TestTrigraphsAndSplices(unittest.TestCase):

    def _assert_trigraph_exit1(self, header, expected_line):
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 1, 2)])
        self.assertEqual(code, 1)
        self.assertIn(f"line {expected_line}: trigraph", err)
        self.assertIn("is not supported", err)

    def test_trigraph_in_line_comment_exit1(self):
        """APLUS-01: ??/ at the end of a // comment would extend the comment."""
        header = (
            "// ??/\n"
            "typedef enum { a = 7 } t_t;\n"
            "typedef enum real { a = 9 } t_t;\n"
        )
        self._assert_trigraph_exit1(header, 1)

    def test_trigraph_in_string_literal_exit1(self):
        """APLUS-01: ??/ in a string would escape the closing quote."""
        header = (
            'const char *s = "??/"typedef enum { a = 7 } t_t;??/"";\n'
            "typedef enum real { a = 9 } t_t;\n"
        )
        self._assert_trigraph_exit1(header, 1)

    def test_trigraph_across_splice_reproduction_exit1(self):
        """APLUS-02: the reproduction is rejected (here for its trigraph)."""
        header = (
            "typedef enum { a = sizeof(\n"
            "/*\n"
            "#define X \\\n"
            '*/ "?\\\n'
            '?-") } t_t;\n'
        )
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 1, 2)])
        self.assertEqual(code, 1)
        self.assertIn("trigraph '??-'", err)

    def test_trigraph_in_enum_body_literal_exit1_not_2(self):
        """A "??-" literal in an enum body is a content error, not a compile error."""
        header = 'typedef enum { a = sizeof("??-") } t_t;'
        self._assert_trigraph_exit1(header, 1)

    def test_every_trigraph_character_rejected_and_line_reported(self):
        for third in "=/'()!<>-":
            with self.subTest(third):
                header = f"\n\n/* ??{third} */\ntypedef enum {{ a }} t_t;\n"
                self._assert_trigraph_exit1(header, 3)

    def test_lone_double_question_mark_accepted(self):
        """?? not followed by a trigraph character is ordinary text."""
        header = "// what??\ntypedef enum { a = sizeof(\"what??\") } t_t; // ??x\n"
        result = gen.extract_enums(header, enums=[("t_t", 1, 2)])
        self.assertEqual(result[0][2], ["a"])

    def test_final_backslash_without_newline_accepted(self):
        """APLUS-03: a backslash at EOF with no newline is not a splice."""
        header = "typedef enum { a } t_t;\n// ends with a backslash \\"
        result = gen.extract_enums(header, enums=[("t_t", 1, 2)])
        self.assertEqual(result[0][2], ["a"])

    def test_final_backslash_crlf_is_a_splice(self):
        """A backslash before CRLF is a splice, same as splice_lines."""
        header = "typedef enum { al\\\r\npha } t_t;"
        code, err = _exit_of(gen.extract_enums, header, enums=[("t_t", 1, 2)])
        self.assertEqual(code, 1)
        self.assertIn("line 1", err)

    def test_backslash_cr_cr_lf_is_not_a_splice(self):
        """splice_lines joins only backslash, optional one CR, LF."""
        header = "typedef enum { a } t_t;\n// c \\\r\r\n"
        result = gen.extract_enums(header, enums=[("t_t", 1, 2)])
        self.assertEqual(result[0][2], ["a"])


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
        code, err = _exit_of(gen.parse_output, "", enum_data, enums=enums)
        self.assertEqual(code, 2)
        self.assertIn('expected 2 rows, got 0', err)

    def test_parse_one_row_missing_exit2(self):
        """Test 8: one row missing from output → exit 2."""
        enums, enum_data = self._enum2()
        output = "my_t\ta\t0\n"  # only the first of two expected rows
        code, err = _exit_of(gen.parse_output, output, enum_data, enums=enums)
        self.assertEqual(code, 2)
        self.assertIn('expected 2 rows, got 1', err)

    def test_parse_rows_reordered_exit2(self):
        """Test 9: two rows in wrong order → completeness check → exit 2."""
        enums, enum_data = self._enum2()
        output = "my_t\tb\t1\nmy_t\ta\t0\n"  # b before a, expected a then b
        code, err = _exit_of(gen.parse_output, output, enum_data, enums=enums)
        self.assertEqual(code, 2)
        self.assertIn('first difference at index 0', err)
        self.assertIn("expected ('my_t', 'a'), got ('my_t', 'b')", err)

    def test_parse_extra_unknown_row_exit2(self):
        """Test 10: extra row with unknown c_name → exit 2."""
        enums, enum_data = self._enum2()
        output = "my_t\ta\t0\nmy_t\tb\t1\nunknown_t\tc\t2\n"
        code, err = _exit_of(gen.parse_output, output, enum_data, enums=enums)
        self.assertEqual(code, 2)
        self.assertIn("unknown enum name in output: 'unknown_t'", err)

    def test_parse_extra_row_in_known_enum_exit2(self):
        """An extra enumerator of a known enum is reported as a completeness failure."""
        enums, enum_data = self._enum2()
        output = "my_t\ta\t0\nmy_t\tb\t1\nmy_t\tc\t2\n"
        code, err = _exit_of(gen.parse_output, output, enum_data, enums=enums)
        self.assertEqual(code, 2)
        self.assertIn("expected 2 rows, got 3", err)
        self.assertIn("first difference at index 2", err)
        self.assertIn("('my_t', 'c')", err)


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


class TestCompileAndRun(unittest.TestCase):

    def test_scratch_dir_permission_error_exit2(self):
        """A PermissionError creating the scratch directory is an I/O error, exit 2."""
        enum_data = [("t_t", " a", ["a"])]
        with mock.patch.object(
            gen.tempfile, "TemporaryDirectory", side_effect=PermissionError("denied")
        ):
            code, err = _exit_of(gen.compile_and_run, enum_data, "cc")
        self.assertEqual(code, 2)
        self.assertIn("scratch directory", err)
        self.assertIn("denied", err)


class TestMain(unittest.TestCase):

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tmp = pathlib.Path(tmp.name)
        self.out = self.tmp / "fixture.tsv"
        self.missing_cls = self.tmp / "no-such-classification.tsv"

    def test_no_classification_skips_validation(self):
        code, stdout, _err = _run_main(
            "--out", self.out, "--classification", self.missing_cls,
            "--no-classification",
        )
        self.assertEqual(code, 0)
        self.assertIn("classification: skipped", stdout)
        self.assertTrue(self.out.read_text(encoding="utf-8").startswith("# Generated by"))

    def test_missing_classification_exit1_without_flag(self):
        code, _out, err = _run_main(
            "--out", self.out, "--classification", self.missing_cls,
        )
        self.assertEqual(code, 1)
        self.assertIn("classification file not found", err)

    def test_failed_run_leaves_existing_out_unchanged(self):
        """Validate-before-write: a classification failure must not touch --out."""
        self.out.write_text("previous fixture\n", encoding="utf-8")
        code, _out, _err = _run_main(
            "--out", self.out, "--classification", self.missing_cls,
        )
        self.assertEqual(code, 1)
        self.assertEqual(self.out.read_text(encoding="utf-8"), "previous fixture\n")

    def test_overlong_out_name_check_mode_exit2(self):
        """An ENAMETOOLONG --out is an I/O error (2), not 'not found' (1)."""
        long_out = self.tmp / ("x" * 300)
        code, _out, err = _run_main(
            "--check", "--out", long_out, "--no-classification",
        )
        self.assertEqual(code, 2)
        self.assertIn("cannot read fixture file", err)
        self.assertNotIn("not found", err)

    def test_overlong_out_name_generate_mode_exit2(self):
        long_out = self.tmp / ("x" * 300)
        code, _out, err = _run_main("--out", long_out, "--no-classification")
        self.assertEqual(code, 2)
        self.assertIn("cannot write fixture file", err)

    def test_check_mode_missing_fixture_exit1(self):
        code, _out, err = _run_main(
            "--check", "--out", self.out, "--no-classification",
        )
        self.assertEqual(code, 1)
        self.assertIn("fixture file not found", err)

    def test_scratch_dir_permission_error_exit2(self):
        with mock.patch.object(
            gen.tempfile, "TemporaryDirectory", side_effect=PermissionError("denied")
        ):
            code, _out, err = _run_main("--out", self.out, "--no-classification")
        self.assertEqual(code, 2)
        self.assertIn("scratch directory", err)
        self.assertFalse(self.out.exists())


if __name__ == "__main__":
    unittest.main()
