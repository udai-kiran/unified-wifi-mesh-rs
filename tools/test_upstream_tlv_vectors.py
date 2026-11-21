"""Tests for the pinned upstream TLV sender and reader vector driver."""
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
DRIVER = ROOT / "tools/upstream-tlv-vectors/regen"
LOADER = importlib.machinery.SourceFileLoader("tlv_vectors_regen", str(DRIVER))
SPEC = importlib.util.spec_from_loader(LOADER.name, LOADER)
regen = importlib.util.module_from_spec(SPEC)
LOADER.exec_module(regen)

FUNCTION = b"short em_t::f(unsigned char *buff)\n{\n    if (buff) {\n        return 1;\n    }\n    return 0;\n}\n"
FUNCTION_ENTRY = ("example.cpp", 1, 7, "short em_t::f(unsigned char *buff)", "em_t")
STRUCT = b"typedef struct {\n    unsigned char a;\n} __attribute__((__packed__)) thing_t;\n"
STRUCT_ENTRY = ("example.h", 1, 3, "typedef struct {", "thing_t")


def fixture_body():
    """The harness output inside the committed fixture (everything from the probe line on)."""
    data = regen.FIXTURE.read_bytes()
    return data[data.index(regen.PROBE_PREFIX.encode()):]


def reader_fixture_body():
    """The harness output inside the committed reader fixture (from its column line on)."""
    data = regen.READER_FIXTURE.read_bytes()
    return data[data.index(regen.READER_COLUMN_LINE.encode()):]


def replace_row(body, case, old, new, function=None):
    """Replace old by new in the row of `case` (and `function`, when several rows share a case)."""
    lines = body.decode().splitlines(keepends=True)
    for index, line in enumerate(lines):
        columns = line.split("\t")
        if columns[0] == case and (function is None or columns[1] == function):
            lines[index] = line.replace(old, new)
            return "".join(lines).encode()
    raise AssertionError(f"no row {case}")


class ExtractionTests(unittest.TestCase):
    def test_function_range_is_extracted_with_the_qualifier_removed(self):
        text = regen.extract_sender(FUNCTION, FUNCTION_ENTRY)
        self.assertIn("short f(unsigned char *buff)\n", text)
        self.assertNotIn("em_t::", text)
        self.assertTrue(text.startswith("// example.cpp:1-7\n"))
        self.assertTrue(text.rstrip().endswith("}"))

    def test_range_start_mismatch_is_rejected(self):
        with self.assertRaises(regen.ContentError):
            regen.extract_sender(FUNCTION.replace(b"short em_t::f", b"short em_t::g"), FUNCTION_ENTRY)
        # A range that starts one line late no longer matches the expected signature.
        shifted = (*FUNCTION_ENTRY[:1], 2, 7, *FUNCTION_ENTRY[3:])
        with self.assertRaises(regen.ContentError):
            regen.extract_sender(FUNCTION, shifted)

    def test_range_past_the_end_of_the_file_is_rejected(self):
        with self.assertRaises(regen.ContentError):
            regen.extract_sender(FUNCTION, (*FUNCTION_ENTRY[:2], 99, *FUNCTION_ENTRY[3:]))

    def test_unbalanced_braces_are_rejected(self):
        missing_close = FUNCTION.replace(b"    }\n", b"")
        extra_close = FUNCTION + b"}\n"
        for name, source, last in (("missing close", missing_close, 6), ("extra close", extra_close, 8)):
            with self.subTest(name), self.assertRaises(regen.ContentError):
                regen.extract_sender(source, (*FUNCTION_ENTRY[:2], last, *FUNCTION_ENTRY[3:]))

    def test_braces_inside_comments_and_literals_are_masked(self):
        source = FUNCTION.replace(b"return 0;", b'return 0; // }\n    const char *s = "}{";')
        text = regen.extract_sender(source, (*FUNCTION_ENTRY[:2], 8, *FUNCTION_ENTRY[3:]))
        self.assertIn('"}{"', text)

    def test_range_ending_early_leaves_a_tail_and_is_rejected(self):
        # Lines 1-7 are the function; a range that also takes a second function is rejected.
        two = FUNCTION + b"int g() { return 0; }\n"
        with self.assertRaises(regen.ContentError):
            regen.extract_sender(two, (*FUNCTION_ENTRY[:2], 8, *FUNCTION_ENTRY[3:]))

    def test_qualifier_is_removed_from_the_declaration_line_only(self):
        source = FUNCTION.replace(b"return 0;", b"return em_t::other();")
        text = regen.extract_sender(source, FUNCTION_ENTRY)
        self.assertIn("short f(unsigned char *buff)", text)
        self.assertIn("return em_t::other();", text)

    def test_qualifier_must_appear_exactly_once(self):
        self.assertEqual(regen.strip_qualifier("int em_t::f()", "em_t"), "int f()")
        for line in ("int f()", "int em_t::em_t::f()", "int other_t::f()"):
            with self.subTest(line=line), self.assertRaises(regen.ContentError):
                regen.strip_qualifier(line, "em_t")

    def test_struct_range_checks_the_packed_attribute_and_name(self):
        self.assertIn("thing_t", regen.extract_struct(STRUCT, STRUCT_ENTRY))
        for broken in (STRUCT.replace(b"thing_t", b"other_t"),
                       STRUCT.replace(b"__attribute__((__packed__)) ", b""),
                       STRUCT.replace(b"typedef struct {", b"typedef struct  {  ").replace(b"{  ", b"{")):
            with self.subTest(broken=broken), self.assertRaises(regen.ContentError):
                regen.extract_struct(broken, STRUCT_ENTRY)

    def test_pinned_ranges_extract_from_the_pin(self):
        sources = regen.read_sources(ROOT.parent / "unified-wifi-mesh")
        with tempfile.TemporaryDirectory() as tmp:
            regen.write_extractions(sources, Path(tmp))
            senders = (Path(tmp) / "senders.inc").read_text()
            structs = (Path(tmp) / "structs.inc").read_text()
        self.assertIn("short create_prof_2_tlv(unsigned char *buff)", senders)
        self.assertIn("unsigned short create_error_code_tlv(", senders)
        self.assertNotIn("::create_", senders)
        self.assertIn("em_profile_2_ap_cap_t;", structs)

    def test_pinned_range_rejects_a_shifted_end(self):
        sources = regen.read_sources(ROOT.parent / "unified-wifi-mesh")
        entry = regen.SENDERS[0]
        for last in (entry[2] - 1, entry[2] + 1):
            with self.subTest(last=last), self.assertRaises(regen.ContentError):
                regen.extract_sender(sources[entry[0]], (*entry[:2], last, *entry[3:]))


class ValidationTests(unittest.TestCase):
    def test_committed_fixture_body_validates(self):
        rows = regen.validate_body(fixture_body())
        self.assertEqual([(row[1], row[0]) for row in rows], regen.expected_cases())

    def test_missing_row_is_rejected(self):
        lines = fixture_body().splitlines(keepends=True)
        with self.assertRaises(regen.ContentError):
            regen.validate_body(b"".join(lines[:-1]))

    def test_duplicate_row_is_rejected(self):
        lines = fixture_body().splitlines(keepends=True)
        with self.assertRaises(regen.ContentError):
            regen.validate_body(b"".join(lines + lines[-1:]))

    def test_inconsistent_rows_are_rejected(self):
        body = fixture_body()
        cases = {
            "return differs from byte count": replace_row(body, "val-1-mac", "\t7\t", "\t6\t"),
            "first octet is not val": replace_row(body, "val-1-mac", "\t01021122", "\t02021122"),
            "NULL MAC not zeroed": replace_row(body, "val-1-null", "01000000000000", "010000000000ff"),
            "MAC octets differ": replace_row(body, "val-1-mac", "021122334455\t7", "021122334456\t7"),
            "bad hex": replace_row(body, "val-1-mac", "01021122334455", "0102112233445g"),
            "wrong profile length": replace_row(body, "default", "\t4\t00001000", "\t3\t000010"),
        }
        for name, broken in cases.items():
            with self.subTest(name), self.assertRaises(regen.ContentError):
                regen.validate_body(broken)

    def test_profile_bytes_match_each_pinned_sender_assignment(self):
        body = fixture_body()
        wrong_values = ["ffffffff", "01001000", "00011000", "00001001"]
        wrong_values += [f"0000{0x10 ^ (1 << bit):02x}00" for bit in range(8)]
        for wrong in wrong_values:
            broken = replace_row(body, "default", "\t4\t00001000", f"\t4\t{wrong}")
            self.assertNotEqual(broken, body)
            with self.assertRaises(regen.ContentError, msg=wrong):
                regen.validate_body(broken)

    def test_case_labels_bind_exact_inputs_even_when_bytes_match(self):
        body = fixture_body()
        rows = [line.split("\t") for line in body.decode().splitlines()
                if line and not line.startswith("#")][1:]
        for case, function, inputs, returned, hex_text in rows:
            changes = [("fill", inputs.replace("fill=ff", "fill=00"), hex_text)]
            if function == "em_configuration_t::create_error_code_tlv":
                fields = dict(pair.split("=", 1) for pair in inputs.split(";"))
                other_value = int(fields["val"]) + 1
                changes.append(("val", inputs.replace(f"val={fields['val']}", f"val={other_value}"),
                                f"{other_value & 0xff:02x}" + hex_text[2:]))
                other_mac = "021122334455" if fields["sta_mac"] == "null" else "null"
                mac_bytes = other_mac if other_mac != "null" else "000000000000"
                changes.append(("null or MAC", inputs.replace(f"sta_mac={fields['sta_mac']}",
                                                             f"sta_mac={other_mac}"),
                                hex_text[:2] + mac_bytes))
                if fields["sta_mac"] != "null":
                    changes.append(("MAC value", inputs.replace(fields["sta_mac"], "0a0b0c0d0e0f"),
                                    hex_text[:2] + "0a0b0c0d0e0f"))
            for changed, new_inputs, new_hex in changes:
                with self.subTest(case=case, changed=changed):
                    old = f"{inputs}\t{returned}\t{hex_text}"
                    new = f"{new_inputs}\t{returned}\t{new_hex}"
                    broken = replace_row(body, case, old, new, function)
                    self.assertNotEqual(broken, body)
                    with self.assertRaises(regen.ContentError):
                        regen.validate_body(broken)

    def test_missing_column_line_is_rejected(self):
        body = fixture_body().replace(regen.COLUMN_LINE.encode() + b"\n", b"")
        with self.assertRaises(regen.ContentError):
            regen.validate_body(body)


class ProbeTests(unittest.TestCase):
    def test_committed_probe_matches_the_expected_layout(self):
        regen.check_probe(fixture_body())
        self.assertEqual(regen.parse_probe(fixture_body()), regen.PROBE_EXPECTED)

    def test_probe_mismatch_is_a_tool_error(self):
        body = fixture_body().replace(b"sizeof=4", b"sizeof=3")
        with self.assertRaises(regen.ToolError):
            regen.check_probe(body)

    def test_missing_probe_line_is_a_tool_error(self):
        body = b"".join(line for line in fixture_body().splitlines(keepends=True)
                        if not line.startswith(regen.PROBE_PREFIX.encode()))
        with self.assertRaises(regen.ToolError):
            regen.check_probe(body)

    def test_malformed_probe_item_is_a_tool_error(self):
        with self.assertRaises(regen.ToolError):
            regen.parse_probe(b"# Probe: thing_t sizeof=x\n")

    def test_duplicate_probe_lines_are_tool_errors(self):
        body = fixture_body()
        probe = body.splitlines(keepends=True)[0]
        for extra in (probe, probe.replace(b"sizeof=4", b"sizeof=3")):
            for duplicate in (extra + body, body + extra):
                with self.assertRaises(regen.ToolError):
                    regen.parse_probe(duplicate)

    def test_empty_and_malformed_probe_lines_are_tool_errors(self):
        for line in (b"# Probe:\n", b"# Probe: \n", b"# Probe: \t \n", b"# Probe:thing_t sizeof=4\n",
                     b"# Probe: thing_t\n", b"# Probe: 7thing sizeof=4\n", b"# Probe: thing_t =4\n",
                     b"# Probe: thing_t bogus=4\n", b"# Probe: thing_t offsetof(a)=0\n",
                     b"# Probe: thing_t sizeof=4 sizeof=4\n", b"# Probe: thing_t sizeof=4 offset(a)=0\n",
                     b"# Probe: thing_t sizeof=" + b"9" * 5000 + b"\n"):
            with self.assertRaises(regen.ToolError, msg=repr(line[:80])):
                regen.parse_probe(line)

    def test_offset_named_sizeof_does_not_replace_the_required_sizeof(self):
        with self.assertRaises(regen.ToolError):
            regen.parse_probe(b"# Probe: thing_t offsetof(sizeof)=4\n")


class DriverTests(unittest.TestCase):
    """Runs main() with the compiler step replaced, so no C++ build is needed."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "fixture.tsv"
        self.reader_out = Path(self.tmp.name) / "readers.tsv"

    def invoke(self, body, *args, reader_body=None, version="fake-c++ 1.0"):
        reader_body = reader_fixture_body() if reader_body is None else reader_body
        compiled = (body, reader_body, version, ["-std=c++17", "-O2"])
        err = io.StringIO()
        with mock.patch.object(regen, "compile_and_run", return_value=compiled), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            code = regen.main(["--out", str(self.out), "--reader-out", str(self.reader_out), *args])
        return code, err.getvalue()

    def test_write_then_check_round_trips(self):
        code, err = self.invoke(fixture_body())
        self.assertEqual((code, err), (0, ""))
        header = self.out.read_text().splitlines()[:3]
        self.assertTrue(header[0].startswith("# Generated by tools/upstream-tlv-vectors/regen"))
        self.assertEqual(self.invoke(fixture_body(), "--check")[0], 0)

    def test_stale_check_is_exit_1_and_does_not_write(self):
        self.invoke(fixture_body())
        before = self.out.read_bytes()
        # Same rows, different compiler version: --check compares the provenance header too.
        code, err = self.invoke(fixture_body(), "--check", version="other-c++ 2.0")
        self.assertEqual(code, 1)
        self.assertIn("fixture differs", err)
        self.assertEqual(self.out.read_bytes(), before)

    def test_check_with_missing_fixture_is_exit_2(self):
        code, err = self.invoke(fixture_body(), "--check")
        self.assertEqual(code, 2)
        self.assertIn("error:", err)
        # Also when only the reader fixture is missing.
        self.invoke(fixture_body())
        self.reader_out.unlink()
        self.assertEqual(self.invoke(fixture_body(), "--check")[0], 2)

    def test_invalid_result_is_rejected_before_write(self):
        broken = replace_row(fixture_body(), "val-1-mac", "\t7\t", "\t6\t")
        code, err = self.invoke(broken)
        self.assertEqual(code, 1)
        self.assertIn("error:", err)
        self.assertFalse(self.out.exists())

    def test_invalid_result_keeps_the_existing_fixture(self):
        self.invoke(fixture_body())
        before = self.out.read_bytes()
        broken = replace_row(fixture_body(), "val-1-mac", "\t7\t", "\t6\t")
        self.assertEqual(self.invoke(broken)[0], 1)
        self.assertEqual(self.out.read_bytes(), before)

    def test_wrong_case_inputs_are_rejected_before_write(self):
        body = fixture_body()
        broken = replace_row(body, "val-1-mac", "val=1;sta_mac=021122334455\t7\t01021122334455",
                             "val=2;sta_mac=021122334455\t7\t02021122334455")
        code, err = self.invoke(broken)
        self.assertEqual(code, 1, err)
        self.assertFalse(self.out.exists())
        self.assertEqual(self.invoke(body)[0], 0)
        before = self.out.read_bytes()
        code, err = self.invoke(broken)
        self.assertEqual(code, 1, err)
        self.assertEqual(self.out.read_bytes(), before)

    def test_probe_mismatch_is_exit_2_and_does_not_write(self):
        code, err = self.invoke(fixture_body().replace(b"sizeof=4", b"sizeof=3"))
        self.assertEqual(code, 2)
        self.assertIn("probe mismatch", err)
        self.assertFalse(self.out.exists())

    def test_invalid_profile_bytes_are_exit_1_and_do_not_replace_fixture(self):
        body = fixture_body()
        self.assertEqual(self.invoke(body)[0], 0)
        before = self.out.read_bytes()
        broken = replace_row(body, "default", "\t4\t00001000", "\t4\tffffffff")
        for mode in ((), ("--check",)):
            code, err = self.invoke(broken, *mode)
            self.assertEqual(code, 1, err)
            self.assertIn("error:", err)
            self.assertEqual(self.out.read_bytes(), before)

    def test_duplicate_or_malformed_probe_is_exit_2_and_does_not_replace_fixture(self):
        body = fixture_body()
        self.assertEqual(self.invoke(body)[0], 0)
        before = self.out.read_bytes()
        probe = body.splitlines(keepends=True)[0]
        for broken in (b"# Probe: \n" + body, b"# Probe:\n" + body, probe + body):
            for mode in ((), ("--check",)):
                code, err = self.invoke(broken, *mode)
                self.assertEqual(code, 2, err)
                self.assertIn("error:", err)
                self.assertEqual(self.out.read_bytes(), before)

    def test_missing_pin_object_is_exit_2(self):
        with tempfile.TemporaryDirectory() as tmp, contextlib.redirect_stderr(io.StringIO()) as err:
            code = regen.main(["--upstream", tmp, "--out", str(self.out)])
        self.assertEqual(code, 2)
        self.assertIn("error:", err.getvalue())
        self.assertFalse(self.out.exists())

    def test_compile_failure_is_exit_2(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code = regen.main(["--cxx", "does-not-exist-c++", "--out", str(self.out)])
        self.assertEqual(code, 2)
        self.assertFalse(self.out.exists())


MACRO = b"#define EM_LIMIT 5\n#define EM_TWO(x, y) \\\n    ((x) + (y))\n#define EM_THREE 3\n"
READER = (b"bool em_msg_t::get(unsigned char *mac)\n{\n    if (m_buff) {\n        return true;\n    }\n"
          b"    return false;\n}\n")
READER_ENTRY = ("example.cpp", 1, 7, "bool em_msg_t::get(unsigned char *mac)", "em_msg_t")
PLAIN_STRUCT = b"typedef struct {\n    int a;\n} thing_t;\n"
PLAIN_ENTRY = ("example.h", 1, 3, "typedef struct {", "thing_t", True)
ENUM = b"typedef enum {\n    one,\n    two,\n} number_t;\n"
ENUM_ENTRY = ("example.h", 1, 4, "typedef enum {", "number_t", False)
GOOD_SHIM = """struct em_msg_t {
    unsigned char *m_buff;
    unsigned int m_len;

    // a comment
    bool get_bss_id(mac_address_t *mac);
    static em_tlv_t *get_first_tlv(em_tlv_t* tlvs_buff, unsigned int buff_len);
};
"""
GOOD_HEADER = """class em_msg_t {
    unsigned char *m_buff;
    unsigned int m_len;
public:
\tstatic em_tlv_t *get_first_tlv(em_tlv_t* tlvs_buff, unsigned int buff_len);
\tbool get_bss_id(mac_address_t *mac);
};
"""


class DefinitionExtractionTests(unittest.TestCase):
    def test_define_ranges_are_extracted_with_their_continuation_lines(self):
        single = ("example.h", 1, 1, "#define EM_LIMIT 5", "EM_LIMIT")
        self.assertIn("#define EM_LIMIT 5\n", regen.extract_define(MACRO, single))
        multi = ("example.h", 2, 3, "#define EM_TWO(x, y) \\", "EM_TWO")
        text = regen.extract_define(MACRO, multi)
        self.assertTrue(text.startswith("// example.h:2-3\n#define EM_TWO(x, y) \\\n"))
        self.assertTrue(text.endswith("((x) + (y))\n"))

    def test_define_range_mismatches_are_rejected(self):
        for entry in (("example.h", 1, 1, "#define EM_LIMIT 6", "EM_LIMIT"),      # other text
                      ("example.h", 2, 2, "#define EM_TWO(x, y) \\", "EM_TWO"),   # cut mid-macro
                      ("example.h", 2, 4, "#define EM_TWO(x, y) \\", "EM_TWO"),   # takes a macro too many
                      ("example.h", 1, 1, "#define EM_LIMIT 5", "EM_OTHER"),     # wrong name
                      ("example.h", 1, 9, "#define EM_LIMIT 5", "EM_LIMIT")):    # past the end
            with self.subTest(entry=entry), self.assertRaises(regen.ContentError):
                regen.extract_define(MACRO, entry)

    def test_plain_typedefs_check_the_name_and_forbid_the_packed_attribute(self):
        self.assertIn("thing_t", regen.extract_plain_type(PLAIN_STRUCT, PLAIN_ENTRY))
        self.assertIn("number_t", regen.extract_plain_type(ENUM, ENUM_ENTRY))
        for broken in (PLAIN_STRUCT.replace(b"thing_t", b"other_t"),
                       PLAIN_STRUCT.replace(b"} thing_t", b"} __attribute__((__packed__)) thing_t")):
            with self.subTest(broken=broken), self.assertRaises(regen.ContentError):
                regen.extract_plain_type(broken, PLAIN_ENTRY)

    def test_packed_attribute_without_a_space_is_accepted(self):
        source = STRUCT.replace(b"} __attribute__((__packed__)) thing_t", b"}__attribute__((__packed__))thing_t")
        self.assertIn("thing_t", regen.extract_struct(source, STRUCT_ENTRY))

    def test_pinned_types_and_macros_extract_from_the_pin(self):
        sources = regen.read_sources(ROOT.parent / "unified-wifi-mesh")
        with tempfile.TemporaryDirectory() as tmp:
            regen.write_extractions(sources, Path(tmp))
            structs = (Path(tmp) / "structs.inc").read_text()
            defines = (Path(tmp) / "defines.inc").read_text()
        for name in ("em_tlv_t", "em_tlv_type_t", "em_profile_type_t", "em_freq_band_t",
                     "em_unassoc_sta_metrics_rsp_t", "em_backhaul_bss_config_policy_t"):
            self.assertIn(f"}} {name};", structs.replace("__attribute__((__packed__)) ", "").replace(
                "__attribute__((__packed__))", ""), name)
        self.assertIn("#define EM_MAX_SERVICE", defines)
        self.assertIn("#define EM_ASSERT_NOT_NULL", defines)
        # Upstream declares em_tlv_t before the structs that follow it; the file order is kept.
        self.assertLess(structs.index("em_tlv_t;"), structs.index("em_ap_radio_basic_cap_t;"))


class ReaderExtractionTests(unittest.TestCase):
    def test_reader_keeps_the_qualifier(self):
        text = regen.extract_reader(READER, READER_ENTRY)
        self.assertIn("bool em_msg_t::get(unsigned char *mac)\n", text)
        self.assertTrue(text.startswith("// example.cpp:1-7\n"))

    def test_reader_range_and_signature_mismatches_are_rejected(self):
        cases = {
            "other signature": (READER.replace(b"em_msg_t::get(", b"em_msg_t::put("), READER_ENTRY),
            "starts one line late": (READER, (*READER_ENTRY[:1], 2, 7, *READER_ENTRY[3:])),
            "ends one line early": (READER, (*READER_ENTRY[:2], 6, *READER_ENTRY[3:])),
            "past the end": (READER, (*READER_ENTRY[:2], 99, *READER_ENTRY[3:])),
            "second function in the range": (READER + b"int g() { return 0; }\n",
                                             (*READER_ENTRY[:2], 8, *READER_ENTRY[3:])),
        }
        for name, (source, entry) in cases.items():
            with self.subTest(name), self.assertRaises(regen.ContentError):
                regen.extract_reader(source, entry)

    def test_reader_signature_needs_exactly_one_qualifier(self):
        for signature in ("bool get(unsigned char *mac)", "bool em_msg_t::em_msg_t::get(int a)"):
            entry = (*READER_ENTRY[:3], signature, "em_msg_t")
            source = READER.replace(READER_ENTRY[3].encode(), signature.encode())
            with self.subTest(signature=signature), self.assertRaises(regen.ContentError):
                regen.extract_reader(source, entry)

    def test_pinned_reader_ranges_extract_from_the_pin(self):
        sources = regen.read_sources(ROOT.parent / "unified-wifi-mesh")
        with tempfile.TemporaryDirectory() as tmp:
            regen.write_extractions(sources, Path(tmp))
            readers = (Path(tmp) / "readers.inc").read_text()
        self.assertEqual(readers.count("em_msg_t::"), len(regen.READERS))
        self.assertIn("bool em_msg_t::get_radio_id(mac_address_t *mac)\n", readers)
        self.assertIn("em_tlv_t *em_msg_t::get_next_tlv(", readers)

    def test_every_pinned_reader_range_rejects_a_shifted_end_or_start(self):
        sources = regen.read_sources(ROOT.parent / "unified-wifi-mesh")
        for entry in regen.READERS:
            for shifted in ((*entry[:2], entry[2] - 1, *entry[3:]), (*entry[:2], entry[2] + 1, *entry[3:]),
                            (entry[0], entry[1] + 1, *entry[2:])):
                with self.subTest(entry=entry[3][:50], shifted=shifted[1:3]), \
                        self.assertRaises(regen.ContentError):
                    regen.extract_reader(sources[entry[0]], shifted)

    def test_pinned_ranges_of_every_kind_reject_a_changed_expected_first_line(self):
        sources = regen.read_sources(ROOT.parent / "unified-wifi-mesh")
        for entry in regen.SENDERS + regen.READERS:
            changed = (*entry[:3], entry[3] + " ", *entry[4:])
            with self.subTest(entry=entry[3][:50]), self.assertRaises(regen.ContentError):
                regen.extract_range(sources[entry[0]], entry[0], entry[1], entry[2], changed[3])


class ShimTests(unittest.TestCase):
    def test_the_committed_shim_matches_the_pinned_class(self):
        sources = regen.read_sources(ROOT.parent / "unified-wifi-mesh")
        shim = (regen.TOOL / "shim.hpp").read_text()
        regen.check_shim_declarations(shim, sources[regen.EM_MSG_H].decode())

    def test_matching_declarations_are_accepted_whatever_the_white_space(self):
        regen.check_shim_declarations(GOOD_SHIM, GOOD_HEADER)

    def test_an_extra_data_member_is_rejected(self):
        for extra in ("    unsigned int m_num_tlv;\n", "    unsigned char m_other;\n"):
            shim = GOOD_SHIM.replace("    unsigned int m_len;\n", "    unsigned int m_len;\n" + extra)
            with self.subTest(extra=extra), self.assertRaises(regen.ContentError):
                regen.check_shim_declarations(shim, GOOD_HEADER)

    def test_a_missing_or_reordered_member_is_rejected(self):
        for shim in (GOOD_SHIM.replace("    unsigned int m_len;\n", ""),
                     GOOD_SHIM.replace("    unsigned char *m_buff;\n    unsigned int m_len;\n",
                                       "    unsigned int m_len;\n    unsigned char *m_buff;\n")):
            with self.assertRaises(regen.ContentError):
                regen.check_shim_declarations(shim, GOOD_HEADER)

    def test_a_member_that_the_pinned_class_does_not_have_is_rejected(self):
        header = GOOD_HEADER.replace("    unsigned int m_len;\n", "    unsigned int m_length;\n")
        with self.assertRaises(regen.ContentError):
            regen.check_shim_declarations(GOOD_SHIM, header)

    def test_a_declaration_missing_from_the_pinned_header_is_rejected(self):
        for shim in (GOOD_SHIM.replace("bool get_bss_id(", "bool get_bss_ids("),
                     GOOD_SHIM.replace("static em_tlv_t", "em_tlv_t"),  # not static upstream
                     GOOD_SHIM.replace("mac_address_t *mac", "unsigned char *mac")):
            with self.subTest(shim=shim), self.assertRaises(regen.ContentError):
                regen.check_shim_declarations(shim, GOOD_HEADER)

    def test_a_shim_without_the_struct_is_rejected(self):
        with self.assertRaises(regen.ContentError):
            regen.check_shim_declarations("struct other_t {\n};\n", GOOD_HEADER)


class ReaderValidationTests(unittest.TestCase):
    def test_committed_reader_body_validates(self):
        rows = regen.validate_reader_body(reader_fixture_body())
        self.assertEqual([(row[1], row[0]) for row in rows], list(regen.expected_reader_cases()))

    def test_corpus_has_every_reader_and_the_documented_quirks(self):
        rows = {(row[1], row[0]): row for row in regen.validate_reader_body(reader_fixture_body())}
        functions = {function for function, _ in rows}
        self.assertEqual(functions, set(regen.WHOLE_AREA_READERS) | set(regen.TYPE_READERS) |
                         {regen.GET_NEXT_TLV, regen.PARSE_PROFILE})
        # Operational BSS with 0 radios continues the walk; Scan Request with 0 radios ends it.
        found = rows[(regen.GET_RADIO_ID, "operational-bss-0-radios-then-radio-id")]
        self.assertEqual(found[4:], ["true", regen.RUID_2])
        for case in ("channel-scan-req-0-radios", "channel-scan-req-0-radios-full-size",
                     "channel-scan-req-0-radios-then-radio-id"):
            self.assertEqual(rows[(regen.GET_RADIO_ID, case)][4:], ["false", "ff" * 6], case)
        # The Supported Service count is stored before a count above EM_MAX_SERVICE is rejected.
        self.assertEqual(rows[(regen.GET_SERVICE, "supported-service-9")][4:],
                         ["false", "09" + "ff" * 8])
        # First of several wins, in frame order.
        self.assertEqual(rows[(regen.GET_AL_MAC, "al-mac-twice")][4:], ["true", regen.MAC_A])
        self.assertEqual(rows[(regen.GET_STA_MAC, "mixed-sta")][4:], ["true", regen.MAC_B])
        self.assertEqual(rows[(regen.GET_STA_MAC, "mixed-sta-reversed")][4:], ["true", regen.MAC_H])
        self.assertEqual(rows[(regen.GET_BSS_ID, "mixed-bss")][4:], ["true", regen.MAC_D])
        self.assertEqual(rows[(regen.GET_BSS_ID, "mixed-bss-reversed")][4:], ["true", regen.MAC_A])
        # Both profile forms.
        self.assertEqual(rows[(regen.GET_PROFILE, "profile-1-octet-p2")][4:], ["true", "02000000"])
        self.assertEqual(rows[(regen.GET_PROFILE, "profile-4-octet-p3")][4:], ["true", "03000000"])

    def test_every_area_type_is_found_by_each_reader_that_lists_it(self):
        rows = {(row[1], row[0]): row for row in regen.validate_reader_body(reader_fixture_body())}
        by_reader = {
            regen.GET_STA_MAC: ("client-info", "client-assoc-event", "bcon-metric-query",
                                "bcon-metric-rsp", "assoc-sta-link-metric"),
            regen.GET_BSS_ID: ("client-info", "client-assoc-event", "ap-metrics", "bssid-tlv"),
            regen.GET_RADIO_ID: ("radio-id", "ap-radio-basic-cap", "ap-radio-advanced-cap", "ht-cap",
                                 "vht-cap", "he-cap", "channel-pref", "channel-sel-resp",
                                 "op-channel-report", "channel-scan-req-1-radio",
                                 "channel-scan-rslt", "radio-metric", "bh-sta-radio-cap",
                                 "operational-bss-1-radio"),
            regen.GET_FREQ_BAND: ("supported-freq-band-5ghz", "autoconf-freq-band-24ghz"),
        }
        for function, cases in by_reader.items():
            for case in cases:
                with self.subTest(function=function, case=case):
                    self.assertEqual(rows[(function, case)][4], "true")

    def test_the_get_tlv_overloads_agree_and_walk_in_frame_order(self):
        rows = {(row[1], row[0]): row for row in regen.validate_reader_body(reader_fixture_body())}
        for (function, case), row in rows.items():
            if function == regen.GET_TLV:
                self.assertEqual(rows[(regen.GET_TLV_IN_BUFFER, case)][4:], row[4:], case)
        self.assertEqual(rows[(regen.GET_TLV, "al-mac-twice@type-01")][4], "0")
        self.assertEqual(rows[(regen.GET_TLV, "device-info-then-al-mac@type-01")][4], "6")
        self.assertEqual(rows[(regen.GET_TLV, "al-mac@type-00")][4], "-")  # EOM is never "found"
        self.assertEqual(rows[(regen.GET_TLV, "al-mac@type-fd")][4], "-")

    def test_missing_duplicate_and_extra_reader_rows_are_rejected(self):
        lines = reader_fixture_body().splitlines(keepends=True)
        for name, broken in (("missing", lines[:-1]), ("duplicate", lines + lines[-1:]),
                             ("missing first", lines[:1] + lines[2:]),
                             ("swapped", lines[:2] + [lines[3], lines[2]] + lines[4:]),
                             ("extra", lines + [lines[1].replace(b"eom-only", b"not-a-case", 1)])):
            with self.subTest(name), self.assertRaises(regen.ContentError):
                regen.validate_reader_body(b"".join(broken))

    def test_wrong_reader_results_are_rejected(self):
        body = reader_fixture_body()
        cases = {
            "wrong return": replace_row(body, "al-mac", "\ttrue\t", "\tfalse\t", regen.GET_AL_MAC),
            "wrong output": replace_row(body, "al-mac", "\ttrue\taabbccddeeff", "\ttrue\taabbccddeefe",
                                        regen.GET_AL_MAC),
            "unwritten output changed": replace_row(body, "eom-only", "\tffffffffffff", "\t000000000000",
                                                    regen.GET_AL_MAC),
            "offset moved": replace_row(body, "device-info-then-al-mac@type-01", "\t6\t-", "\t9\t-",
                                        regen.GET_TLV),
            "bad hex": replace_row(body, "al-mac", "aabbccddeeff\n", "aabbccddeefg\n", regen.GET_AL_MAC),
            "scan request continues": replace_row(body, "channel-scan-req-0-radios-then-radio-id",
                                                  "\tfalse\tffffffffffff", "\ttrue\tb0b1b2b3b4b5",
                                                  regen.GET_RADIO_ID),
            "op bss stops": replace_row(body, "operational-bss-0-radios-then-radio-id",
                                        "\ttrue\tb0b1b2b3b4b5", "\tfalse\tffffffffffff",
                                        regen.GET_RADIO_ID),
            "service count not stored": replace_row(body, "supported-service-9", "\t09ffffffffffffffff",
                                                    "\tffffffffffffffffff", regen.GET_SERVICE),
            "later TLV wins": replace_row(body, "al-mac-twice", "\ttrue\taabbccddeeff",
                                          "\ttrue\t112233445566", regen.GET_AL_MAC),
            "wrong profile": replace_row(body, "profile-1-octet-p2", "\t02000000", "\t01000000",
                                         regen.GET_PROFILE),
            "bad profile accepted": replace_row(body, "profile-value-out-of-range", "\tfalse\t00000000",
                                                "\ttrue\t04000000", regen.GET_PROFILE),
            "parse failure writes": replace_row(body, "value-4-length-1", "\tfalse\tffffffff",
                                                "\tfalse\t00000000", regen.PARSE_PROFILE),
            "band not padded": replace_row(body, "supported-freq-band-5ghz", "\t01ffffff", "\t010000ff",
                                           regen.GET_FREQ_BAND),
            "next TLV skips": replace_row(body, "zero-length-then-al-mac@from-0", "\t3\t-", "\t-\t-",
                                          regen.GET_NEXT_TLV),
        }
        for name, broken in cases.items():
            self.assertNotEqual(broken, body, name)
            with self.subTest(name), self.assertRaises(regen.ContentError):
                regen.validate_reader_body(broken)

    def test_case_labels_bind_the_area_and_the_inputs(self):
        body = reader_fixture_body()
        area = "010006aabbccddeeff000000"
        other_area = "010006aabbccddeefe000000"
        cases = {
            "area bytes": replace_row(body, "al-mac", area, other_area, regen.GET_AL_MAC),
            "len": replace_row(body, "al-mac", "len=12;", "len=13;", regen.GET_AL_MAC),
            "fill": replace_row(body, "al-mac", "fill=ff", "fill=00", regen.GET_AL_MAC),
            "type": replace_row(body, "al-mac@type-01", "type=01", "type=02", regen.GET_TLV),
            "from": replace_row(body, "al-mac@from-0", "from=0", "from=3", regen.GET_NEXT_TLV),
            "value_len": replace_row(body, "value-2-length-1", "value_len=1", "value_len=4",
                                     regen.PARSE_PROFILE),
        }
        for name, broken in cases.items():
            self.assertNotEqual(broken, body, name)
            with self.subTest(name), self.assertRaises(regen.ContentError):
                regen.validate_reader_body(broken)

    def test_area_that_is_not_well_formed_is_rejected(self):
        for data in (b"", b"\x01\x00", b"\x01\x00\x06\xaa", b"\x00\x00\x00\x00", b"\x00\x00\x01\x00"):
            with self.subTest(data=data), self.assertRaises(regen.ContentError):
                regen.parse_area(data)

    def test_model_follows_the_documented_quirks_on_new_areas(self):
        area = regen.build_area([("operational_bss", "00"), ("channel_scan_req", "8000"),
                                 ("radio_id", regen.RUID_1)])
        # The 2-octet Scan Request is shorter than sizeof(em_channel_scan_req_t): the search ends.
        self.assertEqual(regen.model_reader(regen.GET_RADIO_ID, {}, area), ("false", "ff" * 6))
        area = regen.build_area([("operational_bss", "00"), ("radio_id", regen.RUID_1)])
        self.assertEqual(regen.model_reader(regen.GET_RADIO_ID, {}, area), ("true", regen.RUID_1))
        # A corpus TLV that is too short for the field is a corpus bug, not a "not found".
        area = regen.build_area([("client_info", regen.MAC_A)])
        with self.assertRaises(regen.ContentError):
            regen.model_reader(regen.GET_STA_MAC, {}, area)


class SenderRowTests(unittest.TestCase):
    """The independent checks of the five senders added with the readers."""

    def test_new_sender_cases_are_in_the_fixture_in_order(self):
        keys = [(row[1], row[0]) for row in regen.validate_body(fixture_body())]
        self.assertEqual(keys, regen.expected_cases())
        by_function = {}
        for function, case in keys:
            by_function.setdefault(function, []).append(case)
        self.assertEqual(len(by_function[regen.ERROR_CODE_METRICS]), 4)
        self.assertEqual(by_function[regen.CLIENT_INFO], ["default", "swapped"])
        self.assertEqual(len(by_function[regen.BACKHAUL_POLICY]), 5)
        self.assertEqual(len(by_function[regen.UNASSOC_QUERY]), 6)
        self.assertEqual(len(by_function[regen.UNASSOC_RESPONSE]), 5)

    def test_wrong_new_sender_bytes_are_rejected(self):
        body = fixture_body()
        cases = {
            "error code reason": replace_row(body, "found-0-assoc-1", "\t01021122334455",
                                             "\t00021122334455", regen.ERROR_CODE_METRICS),
            "error code follows sta_found": replace_row(body, "found-1-assoc-0", "\t00021122334455",
                                                        "\t01021122334455", regen.ERROR_CODE_METRICS),
            "client info order": replace_row(body, "default", "aabbccddeeff021122334455",
                                             "021122334455aabbccddeeff", regen.CLIENT_INFO),
            "policy flag bits": replace_row(body, "p-one-1-p-two-0", "02112233445580",
                                            "02112233445540", regen.BACKHAUL_POLICY),
            "policy reserved bits": replace_row(body, "p-one-0-p-two-0", "02112233445500",
                                                "0211223344553f", regen.BACKHAUL_POLICY),
            "policy skips null bssid": replace_row(body, "null-bssid", "\t0\t-", "\t7\t00000000000000",
                                                   regen.BACKHAUL_POLICY),
            "query channel count": replace_row(body, "two-channels", "\t24\t7302", "\t24\t7301",
                                               regen.UNASSOC_QUERY),
            "query null op writes": replace_row(body, "null-op", "\t0\t-", "\t2\t7300",
                                                regen.UNASSOC_QUERY),
            "response time delta order": replace_row(body, "one-entry", "24000000015a", "240100000 05a"
                                                     .replace(" ", ""), regen.UNASSOC_RESPONSE),
            "response includes other class": replace_row(body, "op-121-of-mixed", "\t14\t79010a",
                                                         "\t14\t79020a", regen.UNASSOC_RESPONSE),
            "response count": replace_row(body, "op-125-of-mixed", "7d00", "7d01", regen.UNASSOC_RESPONSE),
        }
        for name, broken in cases.items():
            self.assertNotEqual(broken, body, name)
            with self.subTest(name), self.assertRaises(regen.ContentError):
                regen.validate_body(broken)

    def test_expected_bytes_helpers_follow_the_documented_layouts(self):
        self.assertEqual(regen.expected_query_bytes({"op_class": "115", "channels": "36:-,40:021122334455"}),
                         bytes.fromhex("7302") + bytes.fromhex("2400") + bytes.fromhex("28010211223344" + "55"))
        self.assertEqual(regen.expected_response_bytes({"op_class": "115", "entries": "none"}),
                         bytes((115, 0)))
        entry = "021122334455/36/115/90/16909060"
        self.assertEqual(regen.expected_response_bytes({"op_class": "115", "entries": entry}),
                         bytes((115, 1)) + bytes.fromhex("021122334455") + bytes((36,)) +
                         bytes((1, 2, 3, 4)) + bytes((90,)))


class WriteTests(unittest.TestCase):
    def test_nothing_is_replaced_when_a_later_file_cannot_be_staged(self):
        with tempfile.TemporaryDirectory() as tmp:
            first = Path(tmp) / "first.tsv"
            first.write_bytes(b"old")
            blocked = Path(tmp) / "not-a-directory"
            blocked.write_bytes(b"")
            with self.assertRaises(OSError):
                regen.write_all_atomically([(first, b"new"), (blocked / "second.tsv", b"new")])
            self.assertEqual(first.read_bytes(), b"old")
            self.assertEqual(sorted(path.name for path in Path(tmp).iterdir()),
                             ["first.tsv", "not-a-directory"])

    def test_both_files_are_written_and_no_temporary_file_is_left(self):
        with tempfile.TemporaryDirectory() as tmp:
            first, second = Path(tmp) / "a.tsv", Path(tmp) / "b.tsv"
            regen.write_all_atomically([(first, b"one"), (second, b"two")])
            self.assertEqual((first.read_bytes(), second.read_bytes()), (b"one", b"two"))
            self.assertEqual(sorted(path.name for path in Path(tmp).iterdir()), ["a.tsv", "b.tsv"])


class TwoFixtureDriverTests(unittest.TestCase):
    """main() with both fixtures: --check, and validation before any write."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / "senders.tsv"
        self.reader_out = Path(self.tmp.name) / "readers.tsv"

    def invoke(self, body, reader_body, *args, version="fake-c++ 1.0"):
        compiled = (body, reader_body, version, ["-std=c++17", "-O2"])
        err = io.StringIO()
        with mock.patch.object(regen, "compile_and_run", return_value=compiled), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            code = regen.main(["--out", str(self.out), "--reader-out", str(self.reader_out), *args])
        return code, err.getvalue()

    def write_both(self):
        self.assertEqual(self.invoke(fixture_body(), reader_fixture_body())[0], 0)
        return self.out.read_bytes(), self.reader_out.read_bytes()

    def test_both_files_are_written_with_a_header(self):
        senders, readers = self.write_both()
        self.assertTrue(readers.startswith(b"# Generated by tools/upstream-tlv-vectors/regen"))
        self.assertIn(b"# Extract member function (em_msg_t:: kept)", readers)
        self.assertNotIn(b"# Probe:", readers)
        self.assertIn(b"# Probe:", senders)
        self.assertEqual(self.invoke(fixture_body(), reader_fixture_body(), "--check")[0], 0)

    def test_check_names_the_stale_file_and_changes_neither(self):
        senders, readers = self.write_both()
        stale_reader = replace_row(reader_fixture_body(), "al-mac", "\ttrue\taabbccddeeff",
                                   "\ttrue\taabbccddeeff", regen.GET_AL_MAC)
        # A different compiler version changes the header of both files.
        code, err = self.invoke(fixture_body(), reader_fixture_body(), "--check", version="other 1.0")
        self.assertEqual(code, 1)
        self.assertIn(str(self.out), err)
        self.assertIn(str(self.reader_out), err)
        # Only the reader file edited by hand: the sender file is still current.
        self.reader_out.write_bytes(readers + b"# edited\n")
        code, err = self.invoke(fixture_body(), reader_fixture_body(), "--check")
        self.assertEqual(code, 1)
        self.assertIn(str(self.reader_out), err)
        self.assertNotIn(str(self.out), err)
        # Only the sender file edited by hand.
        self.reader_out.write_bytes(readers)
        self.out.write_bytes(senders + b"# edited\n")
        code, err = self.invoke(fixture_body(), reader_fixture_body(), "--check")
        self.assertEqual(code, 1)
        self.assertIn(str(self.out), err)
        self.assertNotIn(str(self.reader_out), err)
        self.assertEqual((self.out.read_bytes(), self.reader_out.read_bytes()),
                         (senders + b"# edited\n", readers))
        self.assertTrue(stale_reader)

    def test_an_invalid_reader_result_writes_neither_file(self):
        broken = replace_row(reader_fixture_body(), "al-mac", "\ttrue\t", "\tfalse\t", regen.GET_AL_MAC)
        code, err = self.invoke(fixture_body(), broken)
        self.assertEqual(code, 1)
        self.assertIn("error:", err)
        self.assertFalse(self.out.exists())
        self.assertFalse(self.reader_out.exists())

    def test_an_invalid_sender_result_writes_neither_file(self):
        broken = replace_row(fixture_body(), "found-0-assoc-1", "\t01021122334455", "\t00021122334455",
                             regen.ERROR_CODE_METRICS)
        code, err = self.invoke(broken, reader_fixture_body())
        self.assertEqual(code, 1)
        self.assertFalse(self.out.exists())
        self.assertFalse(self.reader_out.exists())

    def test_invalid_results_keep_both_existing_files(self):
        senders, readers = self.write_both()
        broken_reader = replace_row(reader_fixture_body(), "al-mac", "\ttrue\t", "\tfalse\t", regen.GET_AL_MAC)
        broken_sender = replace_row(fixture_body(), "default", "\t4\t00001000", "\t4\tffffffff")
        for body, reader_body in ((fixture_body(), broken_reader), (broken_sender, reader_fixture_body())):
            for mode in ((), ("--check",)):
                code, _ = self.invoke(body, reader_body, *mode)
                self.assertEqual(code, 1)
                self.assertEqual((self.out.read_bytes(), self.reader_out.read_bytes()), (senders, readers))

    def test_a_probe_failure_writes_neither_file(self):
        code, err = self.invoke(fixture_body().replace(b"sizeof=4", b"sizeof=3"), reader_fixture_body())
        self.assertEqual(code, 2)
        self.assertFalse(self.out.exists())
        self.assertFalse(self.reader_out.exists())


class EndToEndTests(unittest.TestCase):
    def test_committed_fixture_is_current(self):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as err:
            code = regen.main(["--check"])
        self.assertEqual(code, 0, err.getvalue())


if __name__ == "__main__":
    unittest.main()
