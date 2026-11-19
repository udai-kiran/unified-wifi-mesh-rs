# SPDX-License-Identifier: Apache-2.0
import contextlib
import importlib.machinery
import importlib.util
import io
import pathlib
import subprocess
import tempfile
import unittest
from unittest.mock import patch

_P = pathlib.Path(__file__).resolve().parent / "gen-upstream-msg-tables"
_loader = importlib.machinery.SourceFileLoader("gen_upstream_msg_tables", str(_P))
_spec = importlib.util.spec_from_loader("gen_upstream_msg_tables", _loader)
gen = importlib.util.module_from_spec(_spec)
_loader.exec_module(gen)

TLV_NAMES = {"em_tlv_type_x", "em_tlv_type_y", "em_tlv_eht_operations"}
MSG_NAMES = {"em_msg_type_a", "em_msg_type_b", "em_msg_type_a_resp",
             "em_msg_type_autoconf_wsc"} | {f"em_msg_type_t{i}" for i in range(80)}
EXT = 16
BASE_H = "#define EMEX_MIN_EXT_DEV_INFO_SZ       16\n"


def _stderr_with_buffer():
    """A text stream that also has .buffer, as read_source writes git's stderr there."""
    return io.TextIOWrapper(io.BytesIO(), encoding="utf-8")


def _suppress():
    """Suppress stderr so SystemExit tests do not pollute -v output."""
    return contextlib.redirect_stderr(_stderr_with_buffer())


# ---------------------------------------------------------------------------
# Synthetic source helpers
# ---------------------------------------------------------------------------


def _row(tlv="em_tlv_type_x", req="mandatory", spec="s", min_len="4"):
    return (
        f'    m_tlv_member[m_num_tlv++] = em_tlv_member_t({tlv}, {req}, "{spec}", {min_len});\n'
    )


def _table(name, *rows):
    return f"void em_msg_t::{name}()\n{{\n{''.join(rows)}}}\n\n"


WSC = "em_msg_type_autoconf_wsc"
WSC_PREAMBLE = (
    "tlvs = tlvs + sizeof(em_raw_hdr_t) + sizeof(em_cmdu_t);\n"
    "len = static_cast<unsigned int>(len - (sizeof(em_raw_hdr_t) + sizeof(em_cmdu_t)));\n"
)
WSC_BREAK = "break;\n"


def _wsc_body(extra_pre="", k1="m1", k2="m2", brk=WSC_BREAK):
    """The upstream WSC case body, with optional deviations for tests."""
    return (
        f"{WSC_PREAMBLE}{extra_pre}"
        f"if (em_configuration_t::get_wsc_msg_type(tlvs, len) == em_wsc_msg_type_{k1}) {{\n"
        "    autoconfig_wsc_m1();\n"
        f"}} else if (em_configuration_t::get_wsc_msg_type(tlvs, len) == em_wsc_msg_type_{k2}) {{\n"
        "    autoconfig_wsc_m2();\n"
        f"}}\n{brk}"
    )


def _ctor(cases):
    """cases: list of (message type, [table names] or raw body text) as a switch.

    A table list is rendered as 'name(); break;'; the WSC case with two
    tables is rendered as the exact upstream text.
    """
    out = (
        "em_msg_t::em_msg_t(em_msg_type_t type, em_profile_type_t profile,"
        " unsigned char *tlvs, unsigned int len)\n{\n    m_type = type;\n"
        "    switch (type) {\n"
    )
    for msg_type, calls in cases:
        out += f"        case {msg_type}:\n"
        if isinstance(calls, str):
            out += calls
        elif msg_type == WSC and len(calls) == 2:
            out += _wsc_body()
        else:
            for call in calls:
                out += f"            {call}();\n"
            out += "            break;\n"
    out += '        default:\n            printf("bad\\n");\n            break;\n    }\n}\n'
    return out


def _small_source():
    """Two tables: constant and ternary rules, plus a WSC-free mapping."""
    return (
        _table(
            "first",
            _row("em_tlv_type_x", "mandatory", "spec one", "9"),
            _row("em_tlv_type_y", "(m_profile > em_profile_type_1) ? mandatory:bad", "spec two", "0"),
        )
        + _table(
            "second",
            _row("em_tlv_type_x", "(m_profile > em_profile_type_2) ? optional : bad", "spec three", "65535"),
        )
        + _ctor([("em_msg_type_a", ["first"]), ("em_msg_type_b", ["second"])])
    )


def _extract(source):
    return gen.extract(source, MSG_NAMES, TLV_NAMES, EXT)


def _full_source(row_for=None):
    """Return a source with 70 tables, 69 types, 179 rows, 6 empty tables.

    row_for(table_index, row_index) may return replacement row text.
    """
    names = ["autoconfig_wsc_m1", "autoconfig_wsc_m2"] + [f"tab{i}" for i in range(2, 70)]
    # 6 empty; of the other 64, 51 have 3 rows and 13 have 2: 153 + 26 = 179.
    sizes = [0] * 6 + [3] * 51 + [2] * 13
    text = ""
    for i, (name, size) in enumerate(zip(names, sizes)):
        rows = []
        for r in range(size):
            replaced = row_for(i, r) if row_for else None
            rows.append(replaced if replaced is not None else _row())
        text += _table(name, *rows)
    # Tables 0 and 1 are the WSC pair; the rest map to t2..t69.
    cases = [("em_msg_type_autoconf_wsc", ["autoconfig_wsc_m1", "autoconfig_wsc_m2"])]
    cases += [(f"em_msg_type_t{i}", [f"tab{i}"]) for i in range(2, 70)]
    return text + _ctor(cases)


# ---------------------------------------------------------------------------
# TestStripComments
# ---------------------------------------------------------------------------


class TestStripComments(unittest.TestCase):
    def test_line_comment_removed(self):
        self.assertEqual(gen.strip_comments("a // c\nb"), "a \nb")

    def test_block_comment_becomes_one_space(self):
        self.assertEqual(gen.strip_comments("a/* c */b"), "a b")

    def test_comment_marker_inside_string_kept(self):
        self.assertEqual(gen.strip_comments('"a//b/*c*/"'), '"a//b/*c*/"')


class TestLiteralAwareScan(unittest.TestCase):
    """Strings and char literals must not look like code to the scanner."""

    def test_double_quote_char_literal_does_not_start_string(self):
        self.assertEqual(gen.strip_comments("c = '\"'; // c\nd"), "c = '\"'; \nd")

    def test_comment_marker_inside_char_literal_kept(self):
        self.assertEqual(gen.strip_comments("c = '//'; d"), "c = '//'; d")

    def test_escaped_quote_char_literal(self):
        self.assertEqual(gen.mask_literals("a '\\'' b"), "a '  ' b")

    def test_unterminated_char_literal_dies(self):
        with _suppress(), self.assertRaises(SystemExit) as cm:
            gen.strip_comments("c = 'x;\n")
        self.assertEqual(cm.exception.code, 1)

    def test_mask_blanks_literal_contents_only(self):
        self.assertEqual(gen.mask_literals('f("{x}", \'}\');'), 'f("   ", \' \');')

    def test_brace_in_char_literal_does_not_end_body(self):
        text = "{ c = '}'; d = '{'; \"}\"; }"
        close = gen._matching_brace(gen.mask_literals(text), 0, "x")
        self.assertEqual(close, len(text) - 1)

    def test_declaration_shaped_string_ignored(self):
        other = 'void other()\n{\n    puts("void em_msg_t::fake() {");\n}\n'
        tables, _ = _extract(
            other + _table("t", _row()) + _ctor([("em_msg_type_a", ["t"])])
        )
        self.assertEqual([name for name, _ in tables], ["t"])

    def test_brace_char_literal_in_constructor_does_not_end_it(self):
        src = _table("t", _row()) + _ctor([("em_msg_type_a", ["t"])]).replace(
            "m_type = type;", "m_type = type; char c = '}';"
        )
        tables, mappings = _extract(src)
        self.assertEqual(mappings, [("em_msg_type_a", "-", "t")])

    def test_double_quote_char_literal_in_constructor_extracts(self):
        src = _table("t", _row()) + _ctor([("em_msg_type_a", ["t"])]).replace(
            "m_type = type;", "m_type = type; char q = '\"';"
        )
        tables, mappings = _extract(src)
        self.assertEqual(mappings, [("em_msg_type_a", "-", "t")])

    def test_constructor_signature_in_string_is_not_a_second_constructor(self):
        """R1-08: the constructor is found in masked text."""
        decoy = (
            'void other()\n{\n    puts("em_msg_t::em_msg_t(em_msg_type_t type,'
            " em_profile_type_t profile, unsigned char *tlvs, unsigned int len)"
            ' {");\n}\n'
        )
        src = decoy + _table("t", _row()) + _ctor([("em_msg_type_a", ["t"])])
        _, mappings = _extract(src)
        self.assertEqual(mappings, [("em_msg_type_a", "-", "t")])

    def test_switch_in_string_is_not_a_second_switch(self):
        """R1-08: the constructor's switch is found in masked text."""
        src = _table("t", _row()) + _ctor([("em_msg_type_a", ["t"])]).replace(
            "m_type = type;", 'm_type = type; puts("switch (type) {");'
        )
        _, mappings = _extract(src)
        self.assertEqual(mappings, [("em_msg_type_a", "-", "t")])

    def test_case_label_in_string_is_not_a_label(self):
        """R1-08: case labels are found in masked text."""
        src = _table("t", _row()) + _ctor([("em_msg_type_a", ["t"])]).replace(
            "    switch (type) {\n",
            '    switch (type) {\n        puts("case em_msg_type_zz:");\n',
        )
        _, mappings = _extract(src)
        self.assertEqual(mappings, [("em_msg_type_a", "-", "t")])


UNSUPPORTED_LEXICAL_LINES = {
    "raw string": 'const char* d = R"x(a)x";\n',
    "prefixed raw string": 'const char* d = u8R"x(a)x";\n',
    "prefixed char": "wchar_t c = L'x';\n",
    "prefixed string": 'const char* d = u8"x";\n',
    "digit separator": "int n = 1'000;\n",
    "digraph": "int a %: 1;\n",
    "trigraph": "int a ??= 1;\n",
}


class TestUnsupportedLexing(unittest.TestCase):
    """One lexing rule: splice lines, then reject constructs we do not parse."""

    def _exit_of(self, func, *args):
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
            func(*args)
        return cm.exception.code, buf.getvalue()

    def test_each_construct_rejected_in_source_and_header(self):
        for label, line in UNSUPPORTED_LEXICAL_LINES.items():
            with self.subTest(label, file="source"):
                code, err = self._exit_of(_extract, line + _small_source())
                self.assertEqual(code, 1)
                self.assertIn("unsupported", err)
            with self.subTest(label, file="header"):
                code, err = self._exit_of(gen.read_ext_dev_info_sz, line + BASE_H)
                self.assertEqual(code, 1)
                self.assertIn("unsupported", err)

    def test_raw_string_decoy_around_source_rejected(self):
        """R1-02: a raw string holding a lone quote cannot expose a payload."""
        src = 'const char* doc = R"tag("\n")tag";\n' + _small_source()
        code, err = self._exit_of(_extract, src)
        self.assertEqual(code, 1)
        self.assertIn("raw string", err)

    def test_digit_separators_around_source_rejected(self):
        """R1-02: paired digit separators cannot hide a declaration."""
        src = "int n = 1'000;\n" + _small_source() + "int m = 2'000;\n"
        code, err = self._exit_of(_extract, src)
        self.assertEqual(code, 1)
        self.assertIn("digit separator", err)

    def test_constructs_inside_comments_and_plain_literals_allowed(self):
        """The rejections apply to code only, not to comments or plain literals."""
        src = (
            "// R\"( L'x' 1'000 %: ??=\n/* u8\"x\" <% */\n"
            'const char* d = "%: ??= R\\"(";\nchar c = \'x\';\n' + _small_source()
        )
        tables, _ = _extract(src)
        self.assertEqual([name for name, _ in tables], ["first", "second"])

    def test_line_splice_inside_identifier_is_removed(self):
        """A backslash-newline inside a table name is spliced out first."""
        src = _small_source().replace("void em_msg_t::first()", "void em_msg_t::fi\\\nrst()")
        tables, _ = _extract(src)
        self.assertEqual([name for name, _ in tables], ["first", "second"])

    def test_line_splice_joins_comment_opener(self):
        """A spliced '/' + '/' comments out the rest of the line."""
        src = "/\\\n/ int n = 1'000;\n" + _small_source()
        tables, _ = _extract(src)
        self.assertEqual([name for name, _ in tables], ["first", "second"])


# ---------------------------------------------------------------------------
# TestExtract
# ---------------------------------------------------------------------------


class TestExtract(unittest.TestCase):
    def test_two_tables_constant_and_ternary_rules(self):
        tables, mappings = _extract(_small_source())
        self.assertEqual(
            tables,
            [
                ("first", [
                    ("em_tlv_type_x", "mandatory", 9, "spec one"),
                    ("em_tlv_type_y", "profile>1?mandatory:bad", 0, "spec two"),
                ]),
                ("second", [
                    ("em_tlv_type_x", "profile>2?optional:bad", 65535, "spec three"),
                ]),
            ],
        )
        self.assertEqual(
            mappings,
            [("em_msg_type_a", "-", "first"), ("em_msg_type_b", "-", "second")],
        )

    def test_all_three_words_and_bad(self):
        src = _table("t", _row(req="optional"), _row(req="bad")) + _ctor(
            [("em_msg_type_a", ["t"])]
        )
        tables, _ = _extract(src)
        self.assertEqual([r[1] for r in tables[0][1]], ["optional", "bad"])

    def test_ext_dev_info_sz_minimum(self):
        src = _table("t", _row(min_len="EMEX_MIN_EXT_DEV_INFO_SZ")) + _ctor(
            [("em_msg_type_a", ["t"])]
        )
        self.assertEqual(_extract(src)[0][0][1][0][2], 16)

    def test_sizeof_minimum_with_free_whitespace(self):
        for expr in (
            "static_cast<int>(sizeof(em_tlv_t) + 2)",
            "static_cast < int > ( sizeof ( em_tlv_t )\n + 2 )",
        ):
            src = _table("t", _row(min_len=expr)) + _ctor([("em_msg_type_a", ["t"])])
            self.assertEqual(_extract(src)[0][0][1][0][2], 5, expr)

    def test_unknown_minimum_expression_rejected(self):
        for expr in ("static_cast<int>(sizeof(em_tlv_t) + 3)", "EMEX_OTHER", "4 + 1", "0x10"):
            src = _table("t", _row(min_len=expr)) + _ctor([("em_msg_type_a", ["t"])])
            with self.subTest(expr=expr):
                buf = io.StringIO()
                with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
                    _extract(src)
                self.assertEqual(cm.exception.code, 1)
                self.assertIn("table t row 0", buf.getvalue())

    def test_unsupported_minimum_message_text(self):
        src = _table("t", _row(min_len="EMEX_OTHER")) + _ctor([("em_msg_type_a", ["t"])])
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit):
            _extract(src)
        self.assertIn("unsupported minimum length", buf.getvalue())

    def test_eht_operations_accepted(self):
        src = _table("t", _row(tlv="em_tlv_eht_operations")) + _ctor(
            [("em_msg_type_a", ["t"])]
        )
        self.assertEqual(_extract(src)[0][0][1][0][0], "em_tlv_eht_operations")

    def test_unknown_tlv_name_rejected(self):
        src = _table("t", _row(tlv="em_tlv_type_nope")) + _ctor([("em_msg_type_a", ["t"])])
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
            _extract(src)
        self.assertEqual(cm.exception.code, 1)
        self.assertIn("unknown TLV type 'em_tlv_type_nope'", buf.getvalue())

    def test_whitespace_after_open_paren_accepted(self):
        src = _table(
            "t",
            "    m_tlv_member[m_num_tlv++] = em_tlv_member_t( em_tlv_type_x,\n"
            '        mandatory ,\n  "s" , 4 )\n ;\n',
        ) + _ctor([("em_msg_type_a", ["t"])])
        self.assertEqual(_extract(src)[0][0][1], [("em_tlv_type_x", "mandatory", 4, "s")])

    def test_block_comment_inside_row_is_separator(self):
        src = _table("t", _row(min_len="9/**/1")) + _ctor([("em_msg_type_a", ["t"])])
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
            _extract(src)
        self.assertEqual(cm.exception.code, 1)
        self.assertIn("expected ')' after minimum length", buf.getvalue())

    def test_comments_between_rows_ignored(self):
        src = _table(
            "t",
            "    // m_tlv_member[m_num_tlv++] = em_tlv_member_t(em_tlv_type_x, bad, \"s\", 1);\n",
            _row(),
            "    /* note; { */\n",
        ) + _ctor([("em_msg_type_a", ["t"])])
        self.assertEqual(len(_extract(src)[0][0][1]), 1)

    def test_empty_table_extracted(self):
        src = _table("t") + _ctor([("em_msg_type_a", ["t"])])
        self.assertEqual(_extract(src)[0], [("t", [])])

    def test_duplicate_row_kept(self):
        src = _table("t", _row(), _row()) + _ctor([("em_msg_type_a", ["t"])])
        rows = _extract(src)[0][0][1]
        self.assertEqual(len(rows), 2)
        self.assertEqual(rows[0], rows[1])

    def _wsc_source(self, body):
        return (
            _table("autoconfig_wsc_m1", _row())
            + _table("autoconfig_wsc_m2", _row())
            + _ctor([(WSC, body)])
        )

    def _assert_dies(self, src, text):
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
            _extract(src)
        self.assertEqual(cm.exception.code, 1)
        self.assertIn(text, buf.getvalue())

    def _assert_wsc_rejected(self, body):
        self._assert_dies(self._wsc_source(body), "differs from the pinned form")

    def test_wsc_exact_form_with_odd_whitespace_accepted(self):
        body = (
            "tlvs=tlvs+sizeof ( em_raw_hdr_t )+sizeof(em_cmdu_t) ;\n\n"
            "  len = static_cast < unsigned\tint > ( len -\n(sizeof(em_raw_hdr_t)+sizeof(em_cmdu_t)) );\n"
            "if(em_configuration_t :: get_wsc_msg_type(tlvs,len)==em_wsc_msg_type_m1)\n{"
            "autoconfig_wsc_m1 ( ) ; }\nelse\nif ( em_configuration_t::get_wsc_msg_type ( tlvs , len )"
            " == em_wsc_msg_type_m2 ) { autoconfig_wsc_m2(); }\n  break ;"
        )
        _, mappings = _extract(self._wsc_source(body))
        self.assertEqual(
            mappings,
            [(WSC, "m1", "autoconfig_wsc_m1"), (WSC, "m2", "autoconfig_wsc_m2")],
        )

    def test_wsc_case_maps_m1_and_m2(self):
        src = self._wsc_source(_wsc_body())
        _, mappings = _extract(src)
        self.assertEqual(
            mappings,
            [(WSC, "m1", "autoconfig_wsc_m1"), (WSC, "m2", "autoconfig_wsc_m2")],
        )

    def test_wsc_swapped_guard_constants_rejected(self):
        self._assert_wsc_rejected(_wsc_body(k1="m2", k2="m1"))

    def test_wsc_extra_preamble_statement_rejected(self):
        self._assert_wsc_rejected(_wsc_body(extra_pre="if (len == 0) len = 0;\n"))

    def test_wsc_early_return_rejected(self):
        self._assert_wsc_rejected(_wsc_body(extra_pre="if (true) return;\n"))

    def test_wsc_cast_call_in_preamble_rejected(self):
        self._assert_wsc_rejected(
            _wsc_body(extra_pre="static_cast<void>(this->autoconfig_wsc_m1());\n")
        )

    def test_wsc_missing_break_rejected(self):
        self._assert_wsc_rejected(_wsc_body(brk=""))

    def test_wsc_difference_position_and_excerpt_shown(self):
        body = _wsc_body(brk="break;\nfoo();\n")
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit):
            _extract(self._wsc_source(body))
        self.assertIn(f"position {len(gen.PINNED_WSC_CASE)}: 'foo();'", buf.getvalue())

    def test_wsc_m2_table_not_defined_rejected(self):
        src = _table("autoconfig_wsc_m1", _row()) + _ctor([(WSC, _wsc_body())])
        self._assert_dies(src, "table function autoconfig_wsc_m2 is not defined")

    def test_ordinary_case_free_whitespace_accepted(self):
        src = _table("t", _row()) + _ctor([("em_msg_type_a", "  t ( ) ;\n\t break ;\n")])
        self.assertEqual(_extract(src)[1], [("em_msg_type_a", "-", "t")])

    def test_ordinary_case_missing_break_rejected(self):
        src = _table("t", _row()) + _table("u", _row()) + _ctor(
            [("em_msg_type_a", "t();\n"), ("em_msg_type_b", ["u"])]
        )
        self._assert_dies(src, "case em_msg_type_a: expected exactly 'table(); break;'")

    def test_ordinary_case_extra_call_rejected(self):
        src = _table("t", _row()) + _ctor(
            [("em_msg_type_a", "t();\nthis->foo();\nbreak;\n")]
        )
        self._assert_dies(src, "got 't();this->foo();break;'")

    def test_ordinary_case_call_in_string_rejected(self):
        src = _table("t", _row()) + _ctor(
            [("em_msg_type_a", 'printf("t();"); t(); break;\n')]
        )
        self._assert_dies(src, "case em_msg_type_a: expected exactly 'table(); break;'")

    def test_error_shows_first_80_chars_of_compact(self):
        long_body = "t();" + "x();" * 40 + "break;\n"
        src = _table("t", _row()) + _ctor([("em_msg_type_a", long_body)])
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit):
            _extract(src)
        self.assertIn(repr(("t();" + "x();" * 40)[:80]), buf.getvalue())
        self.assertNotIn("x();x();" * 11, buf.getvalue())

    def test_renamed_mapping(self):
        src = _table("ap_mld_config_rsp", _row()) + _ctor(
            [("em_msg_type_a_resp", ["ap_mld_config_rsp"])]
        )
        self.assertEqual(
            _extract(src)[1], [("em_msg_type_a_resp", "-", "ap_mld_config_rsp")]
        )

    def test_default_case_ignored(self):
        src = _table("t", _row()) + _ctor([("em_msg_type_a", ["t"])])
        self.assertEqual(len(_extract(src)[1]), 1)

    def test_case_with_two_tables_rejected(self):
        src = _table("t", _row()) + _table("u", _row()) + _ctor(
            [("em_msg_type_a", ["t", "u"])]
        )
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
            _extract(src)
        self.assertEqual(cm.exception.code, 1)
        self.assertIn("expected exactly 'table(); break;'", buf.getvalue())
        self.assertIn("got 't();u();break;'", buf.getvalue())

    def test_case_with_no_table_rejected(self):
        src = _table("t", _row()) + _ctor([("em_msg_type_a", ["t"]), ("em_msg_type_b", [])])
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit):
            _extract(src)
        self.assertIn("case em_msg_type_b: expected exactly 'table(); break;'", buf.getvalue())
        self.assertIn("got 'break;'", buf.getvalue())

    def test_call_to_unknown_name_rejected(self):
        src = _table("t", _row()) + _ctor([("em_msg_type_a", ["nope"])])
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit):
            _extract(src)
        self.assertIn("unknown function nope()", buf.getvalue())

    def test_unknown_message_type_rejected(self):
        src = _table("t", _row()) + _ctor([("em_msg_type_nope", ["t"])])
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit):
            _extract(src)
        self.assertIn("unknown message type", buf.getvalue())

    def test_unmapped_table_rejected(self):
        src = _table("t", _row()) + _table("orphan", _row()) + _ctor(
            [("em_msg_type_a", ["t"])]
        )
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
            _extract(src)
        self.assertEqual(cm.exception.code, 1)
        self.assertIn("table orphan is mapped 0 times", buf.getvalue())

    def test_table_mapped_twice_rejected(self):
        src = _table("t", _row()) + _ctor([("em_msg_type_a", ["t"]), ("em_msg_type_b", ["t"])])
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit):
            _extract(src)
        self.assertIn("table t is mapped 2 times", buf.getvalue())

    def test_extra_statement_in_table_body_rejected(self):
        src = _table("t", _row(), "    m_num_tlv = 0;\n") + _ctor([("em_msg_type_a", ["t"])])
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit):
            _extract(src)
        self.assertIn("table t: unexpected text 'm_num_tlv = 0;", buf.getvalue())

    def test_bad_spec_strings_rejected(self):
        for spec in ("", "a\tb", "a\\nb"):
            src = _table("t", _row(spec=spec)) + _ctor([("em_msg_type_a", ["t"])])
            with self.subTest(spec=spec), _suppress(), self.assertRaises(SystemExit) as cm:
                _extract(src)
            self.assertEqual(cm.exception.code, 1)

    def test_bad_requirements_rejected(self):
        for req in ("required", "(m_profile > em_profile_type_3) ? optional:bad",
                    "(m_profile > em_profile_type_1) ? optional", "mandatoryx"):
            src = _table("t", _row(req=req)) + _ctor([("em_msg_type_a", ["t"])])
            with self.subTest(req=req), _suppress(), self.assertRaises(SystemExit) as cm:
                _extract(src)
            self.assertEqual(cm.exception.code, 1)

    def test_leading_zero_minimum_rejected(self):
        src = _table("t", _row(min_len="010")) + _ctor([("em_msg_type_a", ["t"])])
        with _suppress(), self.assertRaises(SystemExit):
            _extract(src)

    def test_malformed_row_in_full_size_source(self):
        """A bad row in a 70-table source is rejected by the row parser, not a count check."""
        full = _full_source()
        _extract(full)  # control: the unmodified source parses
        src = _full_source(lambda t, r: _row(spec="x", min_len="oops") if (t, r) == (10, 1) else None)
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
            _extract(src)
        self.assertEqual(cm.exception.code, 1)
        self.assertIn("table tab10 row 1: unsupported minimum length", buf.getvalue())

    def test_missing_constructor_rejected(self):
        with _suppress(), self.assertRaises(SystemExit):
            _extract(_table("t", _row()))


# ---------------------------------------------------------------------------
# TestReadExtDevInfoSz
# ---------------------------------------------------------------------------


class TestReadExtDevInfoSz(unittest.TestCase):
    def test_value_read(self):
        self.assertEqual(gen.read_ext_dev_info_sz("x\n" + BASE_H + "y\n"), 16)

    def test_wrong_value_rejected(self):
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
            gen.read_ext_dev_info_sz("#define EMEX_MIN_EXT_DEV_INFO_SZ 17\n")
        self.assertEqual(cm.exception.code, 1)
        self.assertIn("is 17, expected 16", buf.getvalue())

    def test_missing_or_duplicate_rejected(self):
        for text in ("", BASE_H + BASE_H):
            with self.subTest(text=text), _suppress(), self.assertRaises(SystemExit) as cm:
                gen.read_ext_dev_info_sz(text)
            self.assertEqual(cm.exception.code, 1)

    def test_non_decimal_rejected(self):
        with _suppress(), self.assertRaises(SystemExit):
            gen.read_ext_dev_info_sz("#define EMEX_MIN_EXT_DEV_INFO_SZ 0x10\n")

    def _assert_read_dies(self, text, message):
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
            gen.read_ext_dev_info_sz(text)
        self.assertEqual(cm.exception.code, 1)
        self.assertIn(message, buf.getvalue())

    def test_include_guard_is_allowed(self):
        text = "#ifndef EM_BASE_H\n#define EM_BASE_H\n" + BASE_H + "#endif\n"
        self.assertEqual(gen.read_ext_dev_info_sz(text), 16)

    def test_define_after_closed_conditional_is_allowed(self):
        text = "#ifdef A\n#endif\n" + BASE_H
        self.assertEqual(gen.read_ext_dev_info_sz(text), 16)

    def test_define_inside_if_0_rejected(self):
        self._assert_read_dies("#if 0\n" + BASE_H + "#endif\n", "inside a preprocessor conditional")

    def test_define_inside_ifdef_within_guard_rejected(self):
        text = "#ifndef G\n#define G\n#ifdef A\n" + BASE_H + "#endif\n#endif\n"
        self._assert_read_dies(text, "inside a preprocessor conditional")

    def test_define_inside_conditional_without_guard_rejected(self):
        self._assert_read_dies("#ifndef A\n" + BASE_H + "#endif\n", "inside a preprocessor conditional")

    def test_undef_after_define_rejected(self):
        self._assert_read_dies(BASE_H + "#undef EMEX_MIN_EXT_DEV_INFO_SZ\n", "is #undef'd")

    def test_second_define_rejected(self):
        self._assert_read_dies(
            BASE_H + "#define EMEX_MIN_EXT_DEV_INFO_SZ 17\n", "found 2"
        )

    def test_other_macro_with_same_prefix_ignored(self):
        text = BASE_H + "#undef EMEX_MIN_EXT_DEV_INFO_SZ_OTHER\n"
        self.assertEqual(gen.read_ext_dev_info_sz(text), 16)

    def test_source_file_define_or_undef_rejected(self):
        for line in ("#define EMEX_MIN_EXT_DEV_INFO_SZ 99\n", "#undef EMEX_MIN_EXT_DEV_INFO_SZ\n"):
            with self.subTest(line=line), _suppress(), self.assertRaises(SystemExit) as cm:
                gen.reject_ext_dev_info_directives("int x;\n" + line, "em_msg.cpp")
            self.assertEqual(cm.exception.code, 1)

    def test_define_after_closed_if_0_rejected(self):
        """R1-03: a define in a later '#if 0' is conditional, guard or not."""
        text = "#ifndef G\n#define G\n#endif\n#if 0\n" + BASE_H + "#endif\n"
        self._assert_read_dies(text, "inside a preprocessor conditional")

    def test_define_in_guard_else_branch_rejected(self):
        text = "#ifndef G\n#define G\n#else\n" + BASE_H + "#endif\n"
        self._assert_read_dies(text, "inside a preprocessor conditional")

    def test_define_in_guard_elif_branch_rejected(self):
        text = "#ifndef G\n#define G\n#elif 0\n" + BASE_H + "#endif\n"
        self._assert_read_dies(text, "inside a preprocessor conditional")

    def test_define_in_guard_elifdef_branch_rejected(self):
        for directive in ("#elifdef A", "#elifndef A"):
            with self.subTest(directive):
                text = "#ifndef G\n#define G\n" + directive + "\n" + BASE_H + "#endif\n"
                self._assert_read_dies(text, "inside a preprocessor conditional")

    def test_define_in_conditional_that_is_not_the_last_endif_rejected(self):
        """The guard must be closed by the file's last #endif."""
        text = "#ifndef G\n#define G\n" + BASE_H + "#endif\n#ifdef A\n#endif\n"
        self._assert_read_dies(text, "inside a preprocessor conditional")

    def test_else_after_the_define_is_allowed(self):
        text = "#ifndef G\n#define G\n" + BASE_H + "#else\n#endif\n"
        self.assertEqual(gen.read_ext_dev_info_sz(text), 16)

    def test_closed_sibling_conditional_inside_guard_is_allowed(self):
        text = "#ifndef G\n#define G\n#if A\n#endif\n" + BASE_H + "#endif\n"
        self.assertEqual(gen.read_ext_dev_info_sz(text), 16)

    def test_define_text_inside_string_literal_not_counted(self):
        """A directive must start its logical line; literal text is not one."""
        text = 'const char* s = "#define EMEX_MIN_EXT_DEV_INFO_SZ 99";\n' + BASE_H
        self.assertEqual(gen.read_ext_dev_info_sz(text), 16)

    def test_spliced_directive_in_header_is_read(self):
        """R1-04: '#def' + backslash-newline + 'ine' is a define."""
        text = "#def\\\nine EMEX_MIN_EXT_DEV_INFO_SZ 16\n"
        self.assertEqual(gen.read_ext_dev_info_sz(text), 16)

    def test_spliced_undef_in_header_rejected(self):
        self._assert_read_dies(BASE_H + "#un\\\ndef EMEX_MIN_EXT_DEV_INFO_SZ\n", "is #undef'd")

    def test_digraph_directives_in_header_rejected(self):
        """R1-04: %:if 0 / %:endif around a define cannot disable it silently."""
        self._assert_read_dies("%:if 0\n" + BASE_H + "%:endif\n", "digraph")

    def test_spliced_undef_in_source_file_rejected(self):
        """R1-04: '#un' + backslash-newline + 'def' is an #undef."""
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
            gen.reject_ext_dev_info_directives(
                "int x;\n#un\\\ndef EMEX_MIN_EXT_DEV_INFO_SZ\n", "em_msg.cpp"
            )
        self.assertEqual(cm.exception.code, 1)
        self.assertIn("must not touch", buf.getvalue())

    def test_digraph_undef_in_source_file_rejected(self):
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
            gen.reject_ext_dev_info_directives(
                "int x;\n%:undef EMEX_MIN_EXT_DEV_INFO_SZ\n", "em_msg.cpp"
            )
        self.assertEqual(cm.exception.code, 1)
        self.assertIn("digraph", buf.getvalue())

    def test_source_file_without_directive_accepted(self):
        gen.reject_ext_dev_info_directives("int x = EMEX_MIN_EXT_DEV_INFO_SZ;\n", "em_msg.cpp")


# ---------------------------------------------------------------------------
# TestValidate
# ---------------------------------------------------------------------------


class TestValidate(unittest.TestCase):
    """Each count check fails on its own; the baseline passes."""

    def _base(self):
        tables, mappings = _extract(_full_source())
        return list(tables), list(mappings)

    def _check_dies(self, tables, mappings, text):
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf), self.assertRaises(SystemExit) as cm:
            gen.validate(tables, mappings)
        self.assertEqual(cm.exception.code, 1)
        self.assertIn(text, buf.getvalue())

    def test_baseline_passes(self):
        gen.validate(*self._base())

    def test_table_count(self):
        # Drop one empty table and its mapping: 69 tables, 68 types, 69 mappings.
        tables, mappings = self._base()
        name = tables[2][0]
        tables = [t for t in tables if t[0] != name]
        self._check_dies(tables, mappings, "expected 70 tables, got 69")

    def test_message_type_count(self):
        # Two mappings share a type: 70 tables, 68 types, 70 mappings, same rows.
        tables, mappings = self._base()
        mappings[-1] = (mappings[-2][0], "-", mappings[-1][2])
        self._check_dies(tables, mappings, "expected 69 message types, got 68")

    def test_mapping_count(self):
        # An extra mapping of an existing type: 69 types, 71 mappings.
        tables, mappings = self._base()
        mappings.append((mappings[-1][0], "-", mappings[-1][2]))
        self._check_dies(tables, mappings, "expected 70 mappings, got 71")

    def test_row_count(self):
        # Move a row from a 3-row table into nothing: 178 rows, 6 empty still.
        tables, mappings = self._base()
        i = next(k for k, (_, rows) in enumerate(tables) if len(rows) == 3)
        tables[i] = (tables[i][0], tables[i][1][:2])
        self._check_dies(tables, mappings, "expected 179 rows, got 178")

    def test_empty_table_count(self):
        # Empty table gets a row, another table loses one: 179 rows, 5 empty.
        tables, mappings = self._base()
        i = next(k for k, (_, rows) in enumerate(tables) if not rows)
        j = next(k for k, (_, rows) in enumerate(tables) if len(rows) == 3)
        tables[i] = (tables[i][0], [tables[j][1][0]])
        tables[j] = (tables[j][0], tables[j][1][:2])
        self._check_dies(tables, mappings, "expected 6 empty tables, got 5")

    def test_min_len_out_of_range(self):
        tables, mappings = self._base()
        i = next(k for k, (_, rows) in enumerate(tables) if rows)
        name, rows = tables[i]
        tables[i] = (name, [(rows[0][0], rows[0][1], 65536, rows[0][3])] + rows[1:])
        self._check_dies(tables, mappings, f"table {name} row 0: min_len 65536")


# ---------------------------------------------------------------------------
# TestBuildFixture
# ---------------------------------------------------------------------------


class TestBuildFixture(unittest.TestCase):
    def test_literal_text_for_tiny_input(self):
        tables = [
            ("first", [
                ("em_tlv_type_x", "mandatory", 9, "spec one"),
                ("em_tlv_type_y", "profile>1?mandatory:bad", 5, "spec two"),
            ]),
            ("quiet", []),
            ("second", [("em_tlv_type_x", "profile>2?optional:bad", 16, "spec three")]),
        ]
        mappings = [
            ("em_msg_type_a", "-", "first"),
            ("em_msg_type_b", "-", "quiet"),
            ("em_msg_type_autoconf_wsc", "m2", "second"),
        ]
        expected = (
            "# Generated by tools/gen-upstream-msg-tables. Do not edit by hand.\n"
            "# Source: rdkcentral/unified-wifi-mesh src/em/em_msg.cpp"
            " and inc/em_base.h @ ba4180ba36748629f10c20babc4320dd69c9db4f\n"
            "# Regenerate: tools/gen-upstream-msg-tables\n"
            "# Check: tools/gen-upstream-msg-tables --check\n"
            "# SPDX-License-Identifier: Apache-2.0\n"
            "msg_type\twsc\ttable\trow\ttlv_type\trule\tmin_len\tspec\n"
            "em_msg_type_a\t-\tfirst\t0\tem_tlv_type_x\tmandatory\t9\tspec one\n"
            "em_msg_type_a\t-\tfirst\t1\tem_tlv_type_y\tprofile>1?mandatory:bad\t5\tspec two\n"
            "em_msg_type_b\t-\tquiet\t-\t-\t-\t-\t-\n"
            "em_msg_type_autoconf_wsc\tm2\tsecond\t0\tem_tlv_type_x\tprofile>2?optional:bad\t16\tspec three\n"
        )
        self.assertEqual(gen.build_fixture(tables, mappings), expected)

    def test_duplicate_rows_and_shared_order(self):
        tables = [("t", [("em_tlv_type_x", "bad", 1, "s")] * 2)]
        text = gen.build_fixture(tables, [("em_msg_type_a", "-", "t")])
        self.assertEqual(text.count("em_msg_type_a\t-\tt\t"), 2)
        self.assertIn("\tt\t1\tem_tlv_type_x\tbad\t1\ts\n", text)


# ---------------------------------------------------------------------------
# TestLoadTypeNames
# ---------------------------------------------------------------------------


class TestLoadTypeNames(unittest.TestCase):
    def test_reads_names_by_type(self):
        text = (
            "# comment\nupstream_type\tupstream_identifier\tvalue\n"
            "em_msg_type_t\tem_msg_type_a\t0x0000\n"
            "em_tlv_type_t\tem_tlv_type_x\t0x01\n"
            "em_freq_band_t\tem_freq_band_24\t0\n"
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            path = pathlib.Path(tmpdir) / "e.tsv"
            path.write_text(text, encoding="utf-8")
            self.assertEqual(
                gen.load_type_names(path), ({"em_msg_type_a"}, {"em_tlv_type_x"})
            )

    def test_missing_file_exits_2(self):
        with tempfile.TemporaryDirectory() as tmpdir, _suppress(), \
                self.assertRaises(SystemExit) as cm:
            gen.load_type_names(pathlib.Path(tmpdir) / "none.tsv")
        self.assertEqual(cm.exception.code, 2)


# ---------------------------------------------------------------------------
# TestReadSource
# ---------------------------------------------------------------------------


class TestReadSource(unittest.TestCase):
    def test_git_oserror_exits_2(self):
        with patch.object(gen.subprocess, "run", side_effect=OSError("no git")), \
                _suppress(), self.assertRaises(SystemExit) as cm:
            gen.read_source("/nowhere", "src/em/em_msg.cpp")
        self.assertEqual(cm.exception.code, 2)

    def test_git_failure_exits_2(self):
        failed = subprocess.CompletedProcess([], 128, stdout=b"", stderr=b"fatal\n")
        with patch.object(gen.subprocess, "run", return_value=failed), \
                _suppress(), self.assertRaises(SystemExit) as cm:
            gen.read_source("/nowhere", "src/em/em_msg.cpp")
        self.assertEqual(cm.exception.code, 2)

    def test_git_command_line(self):
        ok = subprocess.CompletedProcess([], 0, stdout=b"text", stderr=b"")
        with patch.object(gen.subprocess, "run", return_value=ok) as run:
            self.assertEqual(gen.read_source("/up", "inc/em_base.h"), "text")
        self.assertEqual(
            run.call_args.args[0],
            ["git", "--no-replace-objects", "-C", "/up", "show",
             "ba4180ba36748629f10c20babc4320dd69c9db4f:inc/em_base.h"],
        )


# ---------------------------------------------------------------------------
# TestMain
# ---------------------------------------------------------------------------


class TestMain(unittest.TestCase):
    """main() tests patch the git read and the em-types names; no checkout needed."""

    def _run_main(self, argv, source, base=BASE_H):
        files = {gen.SOURCE_PATH: source, gen.BASE_HEADER_PATH: base}
        stdout_buf = io.StringIO()
        stderr_buf = io.StringIO()
        exit_code = 0
        with patch.object(gen, "read_source", side_effect=lambda _up, path: files[path]), \
                patch.object(gen, "load_type_names", return_value=(MSG_NAMES, TLV_NAMES)), \
                patch("sys.argv", argv), \
                contextlib.redirect_stdout(stdout_buf), \
                contextlib.redirect_stderr(stderr_buf):
            try:
                gen.main()
            except SystemExit as exc:
                exit_code = exc.code
        return exit_code, stdout_buf.getvalue(), stderr_buf.getvalue()

    def _correct_bytes(self, source):
        tables, mappings = _extract(source)
        gen.validate(tables, mappings)
        return gen.build_fixture(tables, mappings).encode("utf-8")

    def test_write_mode_success(self):
        source = _full_source()
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "sub" / "out.tsv"
            code, stdout, _ = self._run_main(["gen", "--out", str(out)], source)
            self.assertEqual(code, 0)
            self.assertEqual(
                stdout,
                "ok: 70 tables, 69 message types, 70 mappings, 179 rows, 6 empty\n",
            )
            self.assertEqual(out.read_bytes(), self._correct_bytes(source))
            self.assertEqual(len(out.read_text().splitlines()), 6 + 185)

    def test_check_up_to_date_exits_0(self):
        source = _full_source()
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "f.tsv"
            out.write_bytes(self._correct_bytes(source))
            code, _, _ = self._run_main(["gen", "--check", "--out", str(out)], source)
        self.assertEqual(code, 0)

    def test_check_stale_exits_1_file_unchanged(self):
        source = _full_source()
        stale = self._correct_bytes(source) + b"# stale\n"
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "f.tsv"
            out.write_bytes(stale)
            code, stdout, _ = self._run_main(["gen", "--check", "--out", str(out)], source)
            after = out.read_bytes()
        self.assertEqual(code, 1)
        self.assertIn("+++ <generated>", stdout)
        self.assertEqual(after, stale)

    def test_check_crlf_fixture_is_stale_and_unchanged(self):
        source = _full_source()
        crlf = self._correct_bytes(source).replace(b"\n", b"\r\n")
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "f.tsv"
            out.write_bytes(crlf)
            code, _, _ = self._run_main(["gen", "--check", "--out", str(out)], source)
            after = out.read_bytes()
        self.assertEqual(code, 1)
        self.assertEqual(after, crlf)

    def test_check_missing_exits_1_and_creates_nothing(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "missing.tsv"
            code, _, stderr = self._run_main(
                ["gen", "--check", "--out", str(out)], _full_source()
            )
            exists = out.exists()
        self.assertEqual(code, 1)
        self.assertIn("fixture file not found", stderr)
        self.assertFalse(exists)

    def test_write_mode_failed_validation_leaves_file_unchanged(self):
        original = b"existing fixture content\n"
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "f.tsv"
            out.write_bytes(original)
            # Parses fine, but has 1 table instead of 70.
            source = _table("t", _row()) + _ctor([("em_msg_type_a", ["t"])])
            code, _, stderr = self._run_main(["gen", "--out", str(out)], source)
            after = out.read_bytes()
        self.assertEqual(code, 1)
        self.assertIn("expected 70 tables, got 1", stderr)
        self.assertEqual(after, original)

    def test_write_mode_parse_failure_leaves_file_unchanged(self):
        original = b"existing fixture content\n"
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "f.tsv"
            out.write_bytes(original)
            source = _table("t", _row(min_len="bogus")) + _ctor([("em_msg_type_a", ["t"])])
            code, _, _ = self._run_main(["gen", "--out", str(out)], source)
            after = out.read_bytes()
        self.assertEqual(code, 1)
        self.assertEqual(after, original)

    def test_source_undef_of_ext_dev_info_sz_exits_1(self):
        source = _full_source() + "#undef EMEX_MIN_EXT_DEV_INFO_SZ\n"
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "out.tsv"
            code, _, err = self._run_main(["prog", "--out", str(out)], source)
            self.assertEqual(code, 1)
            self.assertIn("must not touch EMEX_MIN_EXT_DEV_INFO_SZ", err)
            self.assertFalse(out.exists())

    def test_wrong_header_constant_exits_1(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir) / "f.tsv"
            code, _, _ = self._run_main(
                ["gen", "--out", str(out)],
                _full_source(),
                base="#define EMEX_MIN_EXT_DEV_INFO_SZ 15\n",
            )
            exists = out.exists()
        self.assertEqual(code, 1)
        self.assertFalse(exists)

    def test_git_failure_exits_2(self):
        failed = subprocess.CompletedProcess([], 128, stdout=b"", stderr=b"fatal\n")
        stderr_buf = _stderr_with_buffer()
        code = 0
        with tempfile.TemporaryDirectory() as tmpdir, \
                patch.object(gen.subprocess, "run", return_value=failed), \
                patch("sys.argv", ["gen", "--out", str(pathlib.Path(tmpdir) / "f.tsv")]), \
                contextlib.redirect_stderr(stderr_buf):
            try:
                gen.main()
            except SystemExit as exc:
                code = exc.code
        self.assertEqual(code, 2)

    def test_unwritable_output_exits_2(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            blocker = pathlib.Path(tmpdir) / "file"
            blocker.write_text("x")
            out = blocker / "f.tsv"  # parent is a regular file
            code, _, stderr = self._run_main(["gen", "--out", str(out)], _full_source())
        self.assertEqual(code, 2)
        self.assertIn("cannot write fixture file", stderr)

    def test_unreadable_fixture_under_check_exits_2(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            out = pathlib.Path(tmpdir)  # a directory: read_bytes raises OSError
            code, _, stderr = self._run_main(
                ["gen", "--check", "--out", str(out)], _full_source()
            )
        self.assertEqual(code, 2)
        self.assertIn("cannot read fixture file", stderr)


if __name__ == "__main__":
    unittest.main()
