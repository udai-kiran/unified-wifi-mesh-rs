# SPDX-License-Identifier: Apache-2.0
import contextlib
import importlib.machinery
import importlib.util
import io
import os
import pathlib
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
    for region, count in [
        ("", global_n),
        ("US", us_n),
        ("EU", eu_n),
        ("JP", jp_n),
        ("CN", cn_n),
    ]:
        for _ in range(count):
            rows.append((1, 1, 2, 2400, 5, region))
    return rows


def _rows_to_body(rows):
    """Render a list of row tuples as C++ initializer entries (no outer braces)."""
    parts = []
    for op_class, min_chan, max_chan, base_freq, spacing, region in rows:
        parts.append(
            f'    {{{op_class}, {min_chan}, {max_chan}, {base_freq}, {spacing}, "{region}"}}'
        )
    return ",\n".join(parts)


def _make_source(rows):
    """Wrap rows in a minimal C++ source containing the frequency_ranges vector."""
    body = _rows_to_body(rows)
    return f"const std::vector<freq_range> frequency_ranges = {{\n{body}\n}};\n"


_REAL_TABLE = (
    "const std::vector<freq_range> frequency_ranges = {\n"
    '    {81, 1, 13, 2407, 5, ""}\n'
    "};\n"
)


def _exit_of(func, *args):
    """Call func, which must raise SystemExit; return (exit code, stderr text)."""
    stderr = io.StringIO()
    with contextlib.redirect_stderr(stderr):
        try:
            func(*args)
        except SystemExit as exc:
            return exc.code, stderr.getvalue()
    raise AssertionError("expected SystemExit")


def _make_valid_source():
    """Return a valid C++ source with 68 rows (30 global, 11 US, 11 EU, 9 JP, 7 CN)."""
    return _make_source(_make_rows())


# ---------------------------------------------------------------------------
# TestStripComments
# ---------------------------------------------------------------------------


class TestStripComments(unittest.TestCase):
    def test_line_comment_removed(self):
        """// comment is removed up to (not including) the newline."""
        result = gen.strip_comments("a // comment\nb")
        self.assertEqual(result, "a \nb")

    def test_block_comment_removed(self):
        """/* ... */ is removed."""
        result = gen.strip_comments("a /* comment */ b")
        self.assertEqual(result, "a   b")

    def test_comment_with_comma_and_semicolon(self):
        """Commas and semicolons inside // and /* */ comments are stripped."""
        result = gen.strip_comments("x // a, b; c\ny /* d; e, f */ z")
        self.assertEqual(result, "x \ny   z")

    def test_comment_with_braces(self):
        """Braces inside comments are stripped and do not affect brace counts."""
        result = gen.strip_comments("a /* { */ b // }\nc")
        self.assertEqual(result, "a   b \nc")

    def test_block_comment_separates_tokens(self):
        """A block comment acts as a space, so 8/**/1 is not read as 81."""
        result = gen.strip_comments("{8/**/1, 1}")
        self.assertEqual(result, "{8 1, 1}")

    def test_raw_string_rejected(self):
        """A raw string is not parsed: strip_comments exits 1 naming it."""
        code, err = _exit_of(gen.strip_comments, 'R"(a " // b)" x // gone\ny')
        self.assertEqual(code, 1)
        self.assertIn("raw string", err)

    def test_double_slash_inside_string_not_a_comment(self):
        """// inside a quoted string is part of the string, not a comment."""
        result = gen.strip_comments('"a//b"')
        self.assertEqual(result, '"a//b"')

    def test_string_with_quote_in_comment_not_opened(self):
        """A " inside a // comment does not open a string state."""
        # After the comment the x should still be in normal state.
        result = gen.strip_comments('a // "open\nb')
        self.assertEqual(result, "a \nb")


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
        self.assertEqual(
            rows,
            [
                (81, 1, 13, 2407, 5, ""),
                (12, 2, 11, 2412, 10, "US"),
            ],
        )

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

    def test_comment_between_equals_and_brace_accepted(self):
        """A comment between '=' and '{' does not hide the declaration."""
        source = (
            "const std::vector<freq_range> frequency_ranges = /* note */{\n"
            '    {81, 1, 13, 2407, 5, ""}\n'
            "};\n"
        )
        rows = gen.extract(source)
        self.assertEqual(rows, [(81, 1, 13, 2407, 5, "")])

    def test_extra_whitespace_in_declaration_accepted(self):
        """Any whitespace between declaration tokens is accepted."""
        source = (
            "const  std::vector< freq_range >\n"
            "    frequency_ranges  =\n"
            "{\n"
            '    {81, 1, 13, 2407, 5, ""}\n'
            "};\n"
        )
        rows = gen.extract(source)
        self.assertEqual(rows, [(81, 1, 13, 2407, 5, "")])

    def test_raw_string_decoy_before_real_table_rejected(self):
        """A raw string holding a table declaration is rejected, not skipped."""
        decoy = (
            'const char* doc = R"(\n'
            "const std::vector<freq_range> frequency_ranges = {\n"
            '    {9, 9, 9, 9, 9, "ZZ"}\n'
            '};\n)";\n'
        )
        code, err = _exit_of(gen.extract, decoy + _REAL_TABLE)
        self.assertEqual(code, 1)
        self.assertIn("raw string", err)

    def test_raw_string_with_delimiter_and_lone_quote_rejected(self):
        """A raw string with a custom delimiter and a lone quote is rejected."""
        decoy = 'const char* doc = R"x(a " b // c)x";\n'
        code, err = _exit_of(gen.extract, decoy + _REAL_TABLE)
        self.assertEqual(code, 1)
        self.assertIn("raw string", err)

    def test_prefixed_char_literal_hiding_raw_string_rejected(self):
        """R1-01: L'"' must not hide a raw-string decoy from the scanner."""
        source = (
            "wchar_t q = L'\"'; const char* doc = R\"tag(\n"
            ')tag"; wchar_t z = L\'"\'; int p = \'x\';\n' + _make_valid_source()
        )
        code, err = _exit_of(gen.extract, source)
        self.assertEqual(code, 1)
        self.assertIn("unsupported", err)

    def test_unsupported_lexical_constructs_rejected(self):
        """Raw/prefixed literals, digit separators, digraphs, trigraphs exit 1."""
        cases = {
            "raw string": 'const char* d = R"x(a)x";\n',
            "prefixed raw string": 'const char* d = u8R"x(a)x";\n',
            "prefixed char": "wchar_t c = L'x';\n",
            "prefixed string": 'const char* d = u8"x";\n',
            "digit separator": "int n = 1'000;\n",
            "digraph": "int a %: 1;\n",
            "digraph brace": "int a <% 1;\n",
            "trigraph": "int a ??= 1;\n",
        }
        for label, line in cases.items():
            with self.subTest(label):
                code, err = _exit_of(gen.extract, line + _REAL_TABLE)
                self.assertEqual(code, 1)
                self.assertIn("unsupported", err)

    def test_constructs_inside_comments_and_plain_literals_allowed(self):
        """The rejections apply to code only, not to comments or plain literals."""
        source = (
            '// R"( L\'x\' 1\'000 %: ??=\n'
            '/* u8"x" <% */ const char* d = "%: ??= R\\"(";\n'
            "char c = 'x';\n" + _REAL_TABLE
        )
        self.assertEqual(gen.extract(source), [(81, 1, 13, 2407, 5, "")])

    def test_line_splices_removed_before_matching(self):
        """A backslash-newline splits neither the declaration nor a number."""
        source = (
            "const std::vector<freq_range> frequency_\\\nranges = {\n"
            '    {81, 1, 13, 24\\\n07, 5, ""}\n'
            "};\n"
        )
        self.assertEqual(gen.extract(source), [(81, 1, 13, 2407, 5, "")])

    def test_line_splice_joins_comment_opener(self):
        """A spliced '/' + '/' is a line comment, as in C phase 2."""
        source = "/\\\n/ const std::vector<freq_range> frequency_ranges = {\n" + _REAL_TABLE
        self.assertEqual(gen.extract(source), [(81, 1, 13, 2407, 5, "")])

    def test_closing_brace_in_region_string_does_not_end_table(self):
        """R1-08: brace matching reads masked text, so "}" in a string is data."""
        source = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            '    {81, 1, 13, 2407, 5, "}"}\n'
            "};\n"
        )
        self.assertEqual(gen.extract(source), [(81, 1, 13, 2407, 5, "}")])

    def test_opening_brace_in_region_string_does_not_nest(self):
        """R1-08: "{" in a string must not leave the table unclosed."""
        source = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            '    {81, 1, 13, 2407, 5, "{"}\n'
            "};\n"
        )
        self.assertEqual(gen.extract(source), [(81, 1, 13, 2407, 5, "{")])

    def test_brace_char_literal_does_not_end_table(self):
        """R1-08: a '}' char literal before the table does not move its braces."""
        source = "char c = '}';\n" + _REAL_TABLE + "char d = '{';\n"
        self.assertEqual(gen.extract(source), [(81, 1, 13, 2407, 5, "")])

    def test_ordinary_string_decoy_before_real_table_ignored(self):
        """A table declaration inside an ordinary string literal is ignored."""
        decoy = (
            'const char* doc = "const std::vector<freq_range>'
            ' frequency_ranges = {";\n'
        )
        real = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            '    {81, 1, 13, 2407, 5, ""}\n'
            "};\n"
        )
        self.assertEqual(gen.extract(decoy + real), [(81, 1, 13, 2407, 5, "")])

    def test_decoy_only_exit1_not_found(self):
        """A table declaration that only appears in a string literal → not found."""
        source = (
            'const char* doc = "const std::vector<freq_range>'
            ' frequency_ranges = {};";\n'
        )
        stderr_buf = io.StringIO()
        with contextlib.redirect_stderr(stderr_buf):
            with self.assertRaises(SystemExit) as cm:
                gen.extract(source)
        self.assertEqual(cm.exception.code, 1)
        self.assertIn("not found", stderr_buf.getvalue())

    def test_char_literal_quote_before_real_table(self):
        """A '"' char literal does not open a string that hides the table."""
        source = (
            "const char quote = '\"';\n"
            "const std::vector<freq_range> frequency_ranges = {\n"
            '    {81, 1, 13, 2407, 5, ""}\n'
            "};\n"
        )
        self.assertEqual(gen.extract(source), [(81, 1, 13, 2407, 5, "")])

    def test_zero_initializers_exit1(self):
        """No frequency_ranges initializer → exit 1."""
        source = "int x = 0;\n"
        with self.assertRaises(SystemExit) as cm, _suppress():
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
        with self.assertRaises(SystemExit) as cm, _suppress():
            gen.extract(source)
        self.assertEqual(cm.exception.code, 1)

    def test_five_field_entry_exit1(self):
        """Entry with 5 fields (no region) → exit 1 even if body is otherwise valid."""
        source = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            "    {1, 1, 2, 2400, 5}\n"
            "};\n"
        )
        with self.assertRaises(SystemExit) as cm, _suppress():
            gen.extract(source)
        self.assertEqual(cm.exception.code, 1)

    def test_unquoted_region_exit1(self):
        """Entry with an unquoted region token → exit 1."""
        source = (
            "const std::vector<freq_range> frequency_ranges = {\n"
            "    {1, 1, 2, 2400, 5, US}\n"
            "};\n"
        )
        with self.assertRaises(SystemExit) as cm, _suppress():
            gen.extract(source)
        self.assertEqual(cm.exception.code, 1)

    def test_stray_token_between_entries_exit1(self):
        """Stray token between entries → exit 1 naming the offending text."""
        rows = _make_rows()
        entries = []
        for op_class, min_chan, max_chan, base_freq, spacing, region in rows:
            entries.append(
                f'{{{op_class}, {min_chan}, {max_chan}, {base_freq}, {spacing}, "{region}"}}'
            )
        # Insert a stray token after the first entry so the total {} count is
        # still 68 but the parser hits unexpected text.
        entries[0] = entries[0] + ", stray_token"
        body = ",\n    ".join(entries)
        source = (
            f"const std::vector<freq_range> frequency_ranges = {{\n    {body}\n}};\n"
        )
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
        with self.assertRaises(SystemExit) as cm, _suppress():
            gen.extract(source)
        self.assertEqual(cm.exception.code, 1)

    def test_block_comment_inside_number_exit1(self):
        """A block comment inside a number splits it into two tokens, causing a parse error."""
        rows = _make_rows()
        entries = []
        for op_class, min_chan, max_chan, base_freq, spacing, region in rows:
            entries.append(
                f'{{{op_class}, {min_chan}, {max_chan}, {base_freq}, {spacing}, "{region}"}}'
            )
        entries[0] = '{8/**/1, 1, 13, 2407, 5, ""}'
        body = ",\n    ".join(entries)
        source = (
            f"const std::vector<freq_range> frequency_ranges = {{\n    {body}\n}};\n"
        )
        stderr_buf = io.StringIO()
        with contextlib.redirect_stderr(stderr_buf):
            with self.assertRaises(SystemExit) as cm:
                gen.extract(source)
        self.assertEqual(cm.exception.code, 1)
        self.assertIn("expected ',' after integer", stderr_buf.getvalue())

    def test_five_field_entry_in_full_table_exit1(self):
        """A 68-entry table with one 5-field entry is rejected by the entry parser."""
        rows = _make_rows()
        entries = []
        for op_class, min_chan, max_chan, base_freq, spacing, region in rows:
            entries.append(
                f'{{{op_class}, {min_chan}, {max_chan}, {base_freq}, {spacing}, "{region}"}}'
            )
        entries[10] = "{1, 1, 2, 2400, 5}"
        body = ",\n    ".join(entries)
        source = (
            f"const std::vector<freq_range> frequency_ranges = {{\n    {body}\n}};\n"
        )
        stderr_buf = io.StringIO()
        with contextlib.redirect_stderr(stderr_buf):
            with self.assertRaises(SystemExit) as cm:
                gen.extract(source)
        self.assertEqual(cm.exception.code, 1)
        # The fifth integer must be followed by a comma, so the entry parser
        # rejects it here, before any row-count check could run.
        self.assertIn("expected ',' after integer", stderr_buf.getvalue())


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
        # max_chan 300 keeps min_chan <= max_chan, so only the range guard
        # can produce "min_chan 256 out of range".
        rows[0] = (1, 256, 300, 2400, 5, "")
        self._check_dies(rows, "min_chan 256 out of range")

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
        """min_chan > max_chan → exit 1 with the ordering message."""
        rows = self._base()
        rows[0] = (1, 5, 2, 2400, 5, "")
        self._check_dies(rows, "min_chan 5 > max_chan 2")

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

    def test_region_with_trailing_newline_rejected(self):
        """A region 'US\\n' is not two letters (re.match with $ would accept it)."""
        rows = self._base()
        for i, row in enumerate(rows):
            if row[5] == "US":
                rows[i] = (row[0], row[1], row[2], row[3], row[4], "US\n")
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
# TestBuildFixture
# ---------------------------------------------------------------------------


class TestBuildFixture(unittest.TestCase):
    def test_literal_text_global_and_regional_rows(self):
        """Fixture text is pinned literally: header, tab columns, '-' for a global row."""
        rows = [(81, 1, 13, 2407, 5, ""), (12, 1, 11, 2407, 5, "US")]
        expected = (
            "# Generated by tools/gen-upstream-freq-ranges. Do not edit by hand.\n"
            "# Source: rdkcentral/unified-wifi-mesh src/utils/util.cpp"
            " @ ba4180ba36748629f10c20babc4320dd69c9db4f\n"
            "# Regenerate: tools/gen-upstream-freq-ranges\n"
            "# Check: tools/gen-upstream-freq-ranges --check\n"
            "# SPDX-License-Identifier: Apache-2.0\n"
            "op_class\tmin_chan\tmax_chan\tbase_freq\tspacing\tregion\n"
            "81\t1\t13\t2407\t5\t-\n"
            "12\t1\t11\t2407\t5\tUS\n"
        )
        self.assertEqual(gen.build_fixture(rows), expected)

    def test_committed_fixture_first_and_last_rows(self):
        """The committed fixture starts and ends with the upstream rows."""
        fixture_path = (
            pathlib.Path(__file__).resolve().parent.parent
            / "crates/em-utils/tests/data/upstream_freq_ranges.tsv"
        )
        text = fixture_path.read_text(encoding="utf-8")
        lines = text.splitlines()
        self.assertEqual(lines[6], "81\t1\t13\t2407\t5\t-")
        self.assertEqual(lines[-1], "6\t149\t157\t5000\t5\tCN")
        self.assertEqual(len(lines), 74)


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
        self.last_stderr = ""
        exit_code = 0
        with patch.object(gen, "read_source", return_value=source):
            with patch("sys.argv", argv):
                with contextlib.redirect_stdout(stdout_buf):
                    with contextlib.redirect_stderr(stderr_buf):
                        try:
                            gen.main()
                        except SystemExit as exc:
                            exit_code = exc.code
        self.last_stderr = stderr_buf.getvalue()
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
        # Full-size table with spacing=0 on the first row: the row count is
        # right, so validation must fail on the spacing rule itself.
        rows = _make_rows()
        rows[0] = (1, 1, 2, 2400, 0, "")
        source = _make_source(rows)
        original = b"existing fixture content\n"
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "fixture.tsv"
            out.write_bytes(original)

            argv = ["gen-upstream-freq-ranges", "--out", str(out)]
            exit_code, _ = self._run_main(argv, source)

            after_bytes = out.read_bytes()

        self.assertEqual(exit_code, 1)
        self.assertIn("spacing must be > 0", self.last_stderr)
        self.assertEqual(after_bytes, original)

    def test_write_mode_failed_replace_leaves_file_and_no_temp(self):
        """Write mode: a failing os.replace exits 2, keeps the old file, leaves no temp file."""
        source = _make_valid_source()
        original = b"existing fixture content\n"
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "fixture.tsv"
            out.write_bytes(original)
            argv = ["gen-upstream-freq-ranges", "--out", str(out)]
            with patch.object(gen.os, "replace", side_effect=OSError("boom")):
                exit_code, _ = self._run_main(argv, source)
            after_bytes = out.read_bytes()
            leftovers = sorted(p.name for p in pathlib.Path(tmpdir).iterdir())

        self.assertEqual(exit_code, 2)
        self.assertIn("cannot write fixture file", self.last_stderr)
        self.assertEqual(after_bytes, original)
        self.assertEqual(leftovers, ["fixture.tsv"])

    def test_write_mode_success(self):
        """Write mode with valid source → exit 0, file written."""
        source = _make_valid_source()
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "out.tsv"
            argv = ["gen-upstream-freq-ranges", "--out", str(out)]
            exit_code, stdout = self._run_main(argv, source)

            expected = gen.build_fixture(_make_rows()).encode("utf-8")
            self.assertEqual(exit_code, 0)
            self.assertIn("ok: 68 rows", stdout)
            self.assertEqual(out.read_bytes(), expected)
            self.assertEqual(out.stat().st_mode & 0o777, 0o644)

    def test_write_mode_replaces_stale_fixture(self):
        """Write mode over an existing stale file → exact expected bytes."""
        source = _make_valid_source()
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "out.tsv"
            out.write_bytes(b"stale\n")
            argv = ["gen-upstream-freq-ranges", "--out", str(out)]
            exit_code, _ = self._run_main(argv, source)

            expected = gen.build_fixture(_make_rows()).encode("utf-8")
            self.assertEqual(exit_code, 0)
            self.assertEqual(out.read_bytes(), expected)

    def test_check_up_to_date_exits_0(self):
        """--check with current fixture → exit 0."""
        source = _make_valid_source()
        rows = gen.extract(source)
        gen.validate(rows)
        fixture_bytes = gen.build_fixture(rows).encode("utf-8")

        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "fixture.tsv"
            out.write_bytes(fixture_bytes)
            os.utime(out, ns=(1_000_000_000, 1_000_000_000))
            mtime_before = out.stat().st_mtime_ns

            argv = ["gen-upstream-freq-ranges", "--check", "--out", str(out)]
            exit_code, _ = self._run_main(argv, source)

            after_bytes = out.read_bytes()
            mtime_after = out.stat().st_mtime_ns
            leftovers = sorted(p.name for p in pathlib.Path(tmpdir).iterdir())

        self.assertEqual(exit_code, 0)
        self.assertEqual(after_bytes, fixture_bytes)
        self.assertEqual(mtime_after, mtime_before)
        self.assertEqual(leftovers, ["fixture.tsv"])


# ---------------------------------------------------------------------------
# TestWriteAtomically
# ---------------------------------------------------------------------------


class TestWriteAtomically(unittest.TestCase):
    def test_longest_legal_basename_is_written(self):
        """R1-06: the temp name must not depend on the destination name length."""
        with tempfile.TemporaryDirectory() as tmpdir:
            name_max = os.pathconf(tmpdir, "PC_NAME_MAX")
            out = pathlib.Path(tmpdir) / ("a" * name_max)
            gen.write_atomically(out, b"data\n")
            self.assertEqual(out.read_bytes(), b"data\n")
            self.assertEqual([p.name for p in pathlib.Path(tmpdir).iterdir()], [out.name])

    def test_temp_file_uses_short_fixed_prefix(self):
        """R1-06: the temp file lives in the destination directory as .tmp-*."""
        seen = []
        real_replace = os.replace

        def spy(src, dst):
            seen.append(pathlib.Path(src))
            real_replace(src, dst)

        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "fixture.tsv"
            with patch.object(gen.os, "replace", side_effect=spy):
                gen.write_atomically(out, b"x")
        self.assertEqual(len(seen), 1)
        self.assertEqual(seen[0].parent, pathlib.Path(tmpdir))
        self.assertTrue(seen[0].name.startswith(".tmp-"))

    def test_interrupt_after_successful_replace_propagates(self):
        """R1-07: KeyboardInterrupt after the rename stays a KeyboardInterrupt."""
        real_replace = os.replace

        def replace_then_interrupt(src, dst):
            real_replace(src, dst)
            raise KeyboardInterrupt

        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "fixture.tsv"
            out.write_bytes(b"old\n")
            with patch.object(gen.os, "replace", side_effect=replace_then_interrupt):
                with self.assertRaises(KeyboardInterrupt):
                    gen.write_atomically(out, b"new\n")
            self.assertEqual(out.read_bytes(), b"new\n")
            self.assertEqual([p.name for p in pathlib.Path(tmpdir).iterdir()], ["fixture.tsv"])

    def test_interrupt_before_replace_removes_temp_file(self):
        """An interrupt before the rename keeps the old file and removes the temp."""
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "fixture.tsv"
            out.write_bytes(b"old\n")
            with patch.object(gen.os, "replace", side_effect=KeyboardInterrupt):
                with self.assertRaises(KeyboardInterrupt):
                    gen.write_atomically(out, b"new\n")
            self.assertEqual(out.read_bytes(), b"old\n")
            self.assertEqual([p.name for p in pathlib.Path(tmpdir).iterdir()], ["fixture.tsv"])

    def test_cleanup_failure_does_not_mask_original_error(self):
        """R1-07: an error while removing the temp file is not the one raised."""
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "fixture.tsv"
            with patch.object(gen.os, "replace", side_effect=OSError("boom")):
                with patch.object(gen.os, "unlink", side_effect=PermissionError("denied")):
                    with self.assertRaises(OSError) as cm:
                        gen.write_atomically(out, b"new\n")
            self.assertEqual(str(cm.exception), "boom")

    def test_success_does_not_unlink(self):
        """After a successful os.replace nothing is unlinked."""
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "fixture.tsv"
            with patch.object(gen.os, "unlink") as unlink:
                gen.write_atomically(out, b"new\n")
            unlink.assert_not_called()
            self.assertEqual(out.read_bytes(), b"new\n")


if __name__ == "__main__":
    unittest.main()
