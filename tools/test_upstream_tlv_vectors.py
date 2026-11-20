"""Tests for the pinned upstream TLV sender vector driver."""
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


def replace_row(body, case, old, new):
    lines = body.decode().splitlines(keepends=True)
    for index, line in enumerate(lines):
        if line.startswith(case + "\t"):
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
                    broken = replace_row(body, case, old, new)
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

    def invoke(self, body, *args):
        compiled = (body, "fake-c++ 1.0", ["-std=c++17", "-O2"])
        err = io.StringIO()
        with mock.patch.object(regen, "compile_and_run", return_value=compiled), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            code = regen.main(["--out", str(self.out), *args])
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
        compiled = (fixture_body(), "other-c++ 2.0", ["-std=c++17", "-O2"])
        err = io.StringIO()
        with mock.patch.object(regen, "compile_and_run", return_value=compiled), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            code = regen.main(["--out", str(self.out), "--check"])
        self.assertEqual(code, 1)
        self.assertIn("fixture differs", err.getvalue())
        self.assertEqual(self.out.read_bytes(), before)

    def test_check_with_missing_fixture_is_exit_2(self):
        code, err = self.invoke(fixture_body(), "--check")
        self.assertEqual(code, 2)
        self.assertIn("error:", err)

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


class EndToEndTests(unittest.TestCase):
    def test_committed_fixture_is_current(self):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()) as err:
            code = regen.main(["--check"])
        self.assertEqual(code, 0, err.getvalue())


if __name__ == "__main__":
    unittest.main()
