# SPDX-License-Identifier: Apache-2.0
import contextlib
import io
import importlib.machinery
import importlib.util
import pathlib
import sys
import tempfile
import unittest
from unittest.mock import patch

_P = pathlib.Path(__file__).resolve().parent / "gen-upstream-freq-ranges"
_loader = importlib.machinery.SourceFileLoader("gen_upstream_freq_ranges", str(_P))
_spec = importlib.util.spec_from_loader("gen_upstream_freq_ranges", _loader)
gen = importlib.util.module_from_spec(_spec)
_loader.exec_module(gen)


def _suppress():
    """Suppress stderr so SystemExit tests do not pollute -v output."""
    return contextlib.redirect_stderr(io.StringIO())


# ---------------------------------------------------------------------------
# Test helpers
# ---------------------------------------------------------------------------

def _make_rows(global_n=30, us_n=11, eu_n=11, jp_n=9, cn_n=7):
    """Return a list of rows with the given per-region counts.

    Values are minimal but valid: op_class 1, chan 1-2, base 2400, spacing 5.
    """
    rows = []
    for region, count in [("", global_n), ("US", us_n), ("EU", eu_n),
                           ("JP", jp_n), ("CN", cn_n)]:
        for _ in range(count):
            rows.append((1, 1, 2, 2400, 5, region))
    return rows


def _rows_to_body(rows):
    """Render a list of row tuples as C++ initializer entries (no outer braces)."""
    parts = []
    for op_class, min_chan, max_chan, base_freq, spacing, region in rows:
        parts.append(
            f"    {{{op_class}, {min_chan}, {max_chan}, {base_freq}, {spacing}, \"{region}\"}}"
        )
    return ",\n".join(parts)


def _make_source(rows):
    """Wrap rows in a minimal C++ source containing the frequency_ranges vector."""
    body = _rows_to_body(rows)
    return f"const std::vector<freq_range> frequency_ranges = {{\n{body}\n}};\n"


def _make_valid_source():
    """Return a valid C++ source with 68 rows (30 global, 11 US, 11 EU, 9 JP, 7 CN)."""
    return _make_source(_make_rows())


# ---------------------------------------------------------------------------
# TestStripComments
# ---------------------------------------------------------------------------

class TestStripComments(unittest.TestCase):

    def test_line_comment_removed(self):
        """// comment is removed up to (not including) the newline."""
        result = gen.strip_comments('a // comment\nb')
        self.assertEqual(result, 'a \nb')

    def test_block_comment_removed(self):
        """/* ... */ is removed."""
        result = gen.strip_comments('a /* comment */ b')
        self.assertEqual(result, 'a  b')

    def test_comment_with_comma_and_semicolon(self):
        """Commas and semicolons inside // and /* */ comments are stripped."""
        result = gen.strip_comments('x // a, b; c\ny /* d; e, f */ z')
        self.assertEqual(result, 'x \ny  z')

    def test_comment_with_braces(self):
        """Braces inside comments are stripped and do not affect brace counts."""
        result = gen.strip_comments('a /* { */ b // }\nc')
        self.assertEqual(result, 'a  b \nc')

    def test_double_slash_inside_string_not_a_comment(self):
        """// inside a quoted string is part of the string, not a comment."""
        result = gen.strip_comments('"a//b"')
        self.assertEqual(result, '"a//b"')

    def test_string_with_quote_in_comment_not_opened(self):
        """A " inside a // comment does not open a string state."""
        # After the comment the x should still be in normal state.
        result = gen.strip_comments('a // "open\nb')
        self.assertEqual(result, 'a \nb')


# ---------------------------------------------------------------------------
# TestExtract
# ---------------------------------------------------------------------------

class TestExtract(unittest.TestCase):

    def test_valid_small_initializer(self):
        """Extract returns rows with correct field order and exact values."""
        source = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            '    {81, 1, 13, 2407, 5, ""},\n'
            '    {12, 2, 11, 2412, 10, "US"}\n'
            "};\n"
        )
        rows = gen.extract(source)
        self.assertEqual(rows, [
            (81, 1, 13, 2407, 5, ""),
            (12, 2, 11, 2412, 10, "US"),
        ])

    def test_comment_with_comma_semicolon_brace_line(self):
        """// comments containing commas, semicolons and braces are stripped."""
        rows = _make_rows()
        body = _rows_to_body(rows)
        # Insert a noisy comment between entries.
        body = body.replace(
            "},\n    {1",
            "}, // {, ; }\n    {1",
            1,
        )
        source = f"const std::vector<freq_range> frequency_ranges = {{\n{body}\n}};\n"
        result = gen.extract(source)
        self.assertEqual(len(result), 68)

    def test_block_comment_with_comma_semicolon_brace(self):
        """/* */ comments containing commas, semicolons and braces are stripped."""
        rows = _make_rows()
        body = _rows_to_body(rows)
        body = body.replace(
            "},\n    {1",
            "}, /* { ; , */ \n    {1",
            1,
        )
        source = f"const std::vector<freq_range> frequency_ranges = {{\n{body}\n}};\n"
        result = gen.extract(source)
        self.assertEqual(len(result), 68)

    def test_double_slash_inside_region_string_not_comment(self):
        """A // inside a quoted region string does not start a line comment."""
        # Build one entry where the region string contains //, so after stripping
        # it must still be present and cause a region-validation error (not a
        # parse error from a swallowed closing brace).
        source = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            '    {1, 1, 2, 2400, 5, "//"},\n'
            "};\n"
        )
        # extract() succeeds (1 row extracted), but the region "//" is invalid
        # and validate() will reject it.  The key is that extract() does not die.
        with _suppress():
            rows = gen.extract(source)
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0][5], "//")

    def test_trailing_comma_accepted(self):
        """An optional trailing comma after the last entry is accepted."""
        source = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            '    {1, 1, 2, 2400, 5, ""},\n'
            "};\n"
        )
        rows = gen.extract(source)
        self.assertEqual(len(rows), 1)

    def test_no_trailing_comma_accepted(self):
        """No trailing comma after the last entry is also accepted."""
        source = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            '    {1, 1, 2, 2400, 5, ""}\n'
            "};\n"
        )
        rows = gen.extract(source)
        self.assertEqual(len(rows), 1)

    def test_zero_initializers_exit1(self):
        """No frequency_ranges initializer → exit 1."""
        source = "int x = 0;\n"
        with self.assertRaises(SystemExit) as cm:
            with _suppress():
                gen.extract(source)
        self.assertEqual(cm.exception.code, 1)

    def test_two_initializers_exit1(self):
        """Two frequency_ranges initializers → exit 1."""
        single = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            '    {1, 1, 2, 2400, 5, ""}\n'
            "};\n"
        )
        source = single + "\n" + single
        with self.assertRaises(SystemExit) as cm:
            with _suppress():
                gen.extract(source)
        self.assertEqual(cm.exception.code, 1)

    def test_five_field_entry_exit1(self):
        """Entry with 5 fields (no region) → exit 1 even if body is otherwise valid."""
        source = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            "    {1, 1, 2, 2400, 5}\n"
            "};\n"
        )
        with self.assertRaises(SystemExit) as cm:
            with _suppress():
                gen.extract(source)
        self.assertEqual(cm.exception.code, 1)

    def test_unquoted_region_exit1(self):
        """Entry with an unquoted region token → exit 1."""
        source = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            "    {1, 1, 2, 2400, 5, US}\n"
            "};\n"
        )
        with self.assertRaises(SystemExit) as cm:
            with _suppress():
                gen.extract(source)
        self.assertEqual(cm.exception.code, 1)

    def test_stray_token_between_entries_exit1(self):
        """Stray token between entries → exit 1 naming the offending text."""
        rows = _make_rows()
        entries = []
        for op_class, min_chan, max_chan, base_freq, spacing, region in rows:
            entries.append(
                f"{{{op_class}, {min_chan}, {max_chan}, {base_freq}, {spacing}, \"{region}\"}}"
            )
        # Insert a stray token after the first entry so the total {} count is
        # still 68 but the parser hits unexpected text.
        entries[0] = entries[0] + ", stray_token"
        body = ",\n    ".join(entries)
        source = f"const std::vector<freq_range> frequency_ranges = {{\n    {body}\n}};\n"
        stderr_buf = io.StringIO()
        with contextlib.redirect_stderr(stderr_buf):
            with self.assertRaises(SystemExit) as cm:
                gen.extract(source)
        self.assertEqual(cm.exception.code, 1)
        # The error message must name the offending text.
        self.assertIn("stray_token", stderr_buf.getvalue())

    def test_leading_zero_integer_exit1(self):
        """Integer with a leading zero (octal in C) → exit 1."""
        source = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            '    {01, 1, 2, 2400, 5, ""}\n'
            "};\n"
        )
        with self.assertRaises(SystemExit) as cm:
            with _suppress():
                gen.extract(source)
        self.assertEqual(cm.exception.code, 1)


# ---------------------------------------------------------------------------
# TestValidate
# ---------------------------------------------------------------------------

class TestValidate(unittest.TestCase):
    """Each rule in validate() is exercised with a targeted bad input."""

    def _base(self):
        """Return a copy of the valid 68-row list."""
        return list(_make_rows())

    def _check_dies(self, rows, keyword=None):
        """Assert validate(rows) calls die (exit 1), optionally checking stderr."""
        stderr_buf = io.StringIO()
        with contextlib.redirect_stderr(stderr_buf):
            with self.assertRaises(SystemExit) as cm:
                gen.validate(rows)
        self.assertEqual(cm.exception.code, 1)
        if keyword:
            self.assertIn(keyword, stderr_buf.getvalue())

    def test_valid_rows_pass(self):
        """Control: the 68-row baseline passes validate()."""
        gen.validate(self._base())  # must not raise

    def test_op_class_too_large(self):
        """op_class > 255 → exit 1 with 'op_class' in message."""
        rows = self._base()
        rows[0] = (256, 1, 2, 2400, 5, "")
        self._check_dies(rows, "op_class")

    def test_min_chan_too_large(self):
        """min_chan > 255 → exit 1 with 'min_chan' in message."""
        rows = self._base()
        rows[0] = (1, 256, 2, 2400, 5, "")
        self._check_dies(rows, "min_chan")

    def test_max_chan_too_large(self):
        """max_chan > 255 → exit 1 with 'max_chan' in message."""
        rows = self._base()
        rows[0] = (1, 1, 256, 2400, 5, "")
        self._check_dies(rows, "max_chan")

    def test_base_freq_too_large(self):
        """base_freq > 65535 → exit 1 with 'base_freq' in message."""
        rows = self._base()
        rows[0] = (1, 1, 2, 65536, 5, "")
        self._check_dies(rows, "base_freq")

    def test_spacing_too_large(self):
        """spacing > 65535 → exit 1 with 'spacing' in message."""
        rows = self._base()
        rows[0] = (1, 1, 2, 2400, 65536, "")
        self._check_dies(rows, "spacing")

    def test_min_chan_greater_than_max_chan(self):
        """min_chan > max_chan → exit 1."""
        rows = self._base()
        rows[0] = (1, 5, 2, 2400, 5, "")
        self._check_dies(rows, "min_chan")

    def test_spacing_zero(self):
        """spacing == 0 → exit 1 with 'spacing' in message."""
        rows = self._base()
        rows[0] = (1, 1, 2, 2400, 0, "")
        self._check_dies(rows, "spacing")

    def test_invalid_region_lowercase(self):
        """Lowercase region (e.g. 'us') → exit 1 with 'region' in message."""
        rows = self._base()
        # Replace a US row with one that has lowercase region.
        for i, row in enumerate(rows):
            if row[5] == "US":
                rows[i] = (row[0], row[1], row[2], row[3], row[4], "us")
                break
        self._check_dies(rows, "region")

    def test_invalid_region_three_letters(self):
        """Three-letter region → exit 1 with 'region' in message."""
        rows = self._base()
        for i, row in enumerate(rows):
            if row[5] == "US":
                rows[i] = (row[0], row[1], row[2], row[3], row[4], "USA")
                break
        self._check_dies(rows, "region")

    def test_wrong_total_count(self):
        """67 rows instead of 68 → exit 1 naming 'expected 68 rows' (not a region)."""
        rows = self._base()[:-1]  # drop last row
        self._check_dies(rows, "expected 68 rows")

    def test_wrong_global_count(self):
        """29 global rows instead of 30 → exit 1 with 'global' in message."""
        # Drop one global row.
        rows = self._base()
        for i, row in enumerate(rows):
            if row[5] == "":
                del rows[i]
                break
        # Now 67 rows total; add one US to keep total at 68 but break global count.
        rows.append((1, 1, 2, 2400, 5, "US"))
        self._check_dies(rows, "global")

    def test_wrong_us_count(self):
        """10 US rows instead of 11 → exit 1 with 'US' in message."""
        rows = self._base()
        for i, row in enumerate(rows):
            if row[5] == "US":
                del rows[i]
                break
        # Replace with an EU row so total stays 68 and global stays 30;
        # US=10 fails before EU=12 because EXPECTED_COUNTS iterates "" then "US".
        rows.append((1, 1, 2, 2400, 5, "EU"))
        self._check_dies(rows, "US")


# ---------------------------------------------------------------------------
# TestMain
# ---------------------------------------------------------------------------

class TestMain(unittest.TestCase):
    """main() tests use a patched read_source so no upstream checkout is needed."""

    def _run_main(self, argv, source, *, expect_exit=None, capture_stdout=False):
        """Call gen.main() with sys.argv=argv and mocked read_source=source.

        Returns (exit_code, stdout_text).
        """
        stdout_buf = io.StringIO()
        stderr_buf = io.StringIO()
        exit_code = 0
        with patch.object(gen, "read_source", return_value=source):
            with patch("sys.argv", argv):
                with contextlib.redirect_stdout(stdout_buf):
                    with contextlib.redirect_stderr(stderr_buf):
                        try:
                            gen.main()
                        except SystemExit as exc:
                            exit_code = exc.code
        return exit_code, stdout_buf.getvalue()

    def test_check_missing_fixture_exits_1(self):
        """--check with no fixture file → exit 1."""
        source = _make_valid_source()
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "missing.tsv"
            argv = ["gen-upstream-freq-ranges", "--check", "--out", str(out)]
            exit_code, _ = self._run_main(argv, source)
        self.assertEqual(exit_code, 1)

    def test_check_stale_fixture_exits_1_file_unchanged(self):
        """--check with stale fixture → exit 1, diff on stdout, file byte-identical."""
        source = _make_valid_source()
        # Build the correct fixture bytes first.
        rows = gen.extract(source)
        gen.validate(rows)
        correct_text = gen.build_fixture(rows)
        correct_bytes = correct_text.encode("utf-8")

        # Write a stale version (append a line).
        stale_bytes = correct_bytes + b"# stale extra line\n"

        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "fixture.tsv"
            out.write_bytes(stale_bytes)

            argv = ["gen-upstream-freq-ranges", "--check", "--out", str(out)]
            exit_code, stdout = self._run_main(argv, source)

            after_bytes = out.read_bytes()

        self.assertEqual(exit_code, 1)
        # The diff should mention the generated side.
        self.assertIn("+++ <generated>", stdout)
        # The file must be byte-identical to what we wrote.
        self.assertEqual(after_bytes, stale_bytes)

    def test_write_mode_failed_validation_leaves_file_unchanged(self):
        """Write mode: validation failure leaves an existing fixture byte-identical."""
        # Source with spacing=0 on first row: passes extraction, fails validation.
        source = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            '    {1, 1, 2, 2400, 0, ""}\n'
            "};\n"
        )
        original = b"existing fixture content\n"
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "fixture.tsv"
            out.write_bytes(original)

            argv = ["gen-upstream-freq-ranges", "--out", str(out)]
            exit_code, _ = self._run_main(argv, source)

            after_bytes = out.read_bytes()

        self.assertEqual(exit_code, 1)
        self.assertEqual(after_bytes, original)

    def test_write_mode_success(self):
        """Write mode with valid source → exit 0, file written."""
        source = _make_valid_source()
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "out.tsv"
            argv = ["gen-upstream-freq-ranges", "--out", str(out)]
            exit_code, stdout = self._run_main(argv, source)

            self.assertEqual(exit_code, 0)
            self.assertIn("ok: 68 rows", stdout)
            self.assertTrue(out.exists())

    def test_check_up_to_date_exits_0(self):
        """--check with current fixture → exit 0."""
        source = _make_valid_source()
        rows = gen.extract(source)
        gen.validate(rows)
        fixture_bytes = gen.build_fixture(rows).encode("utf-8")

        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "fixture.tsv"
            out.write_bytes(fixture_bytes)

            argv = ["gen-upstream-freq-ranges", "--check", "--out", str(out)]
            exit_code, _ = self._run_main(argv, source)

        self.assertEqual(exit_code, 0)


if __name__ == "__main__":
    unittest.main()
