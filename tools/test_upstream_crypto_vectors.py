"""Tests for the pinned upstream crypto fixture driver."""
# SPDX-License-Identifier: Apache-2.0

import contextlib
import importlib.machinery
import importlib.util
import io
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
DRIVER = ROOT / "tools/upstream-crypto-vectors/regen"
LOADER = importlib.machinery.SourceFileLoader("crypto_vectors_regen", str(DRIVER))
SPEC = importlib.util.spec_from_loader(LOADER.name, LOADER)
regen = importlib.util.module_from_spec(SPEC)
LOADER.exec_module(regen)


class RangeTests(unittest.TestCase):
    def test_range_checks_outer_brace_and_signature(self):
        entry = ("example.cpp", 1, 4, "int f() {", "definitions", "function")
        good = b"int f() {\n    if (1) {\n        return 1; }\n}\n"
        self.assertEqual(regen.extract_range(good, entry), good.decode())
        with self.assertRaises(regen.ContentError):
            regen.extract_range(good.replace(b"int f()", b"int g()"), entry)
        with self.assertRaises(regen.ContentError):
            regen.extract_range(good.replace(b"    if (1) {", b"    if (1) { }"), entry)
        with self.assertRaises(regen.ContentError):
            regen.extract_range(good.replace(b"}\n", b";\n", 1), entry)

    def test_validation_rejects_missing_rows_and_bad_hex(self):
        fixture = regen.FIXTURE.read_bytes()
        body = fixture[fixture.index(b"# OpenSSL: ") :]
        lines = body.splitlines(keepends=True)
        regen.validate_body(body)
        with self.assertRaises(regen.ContentError):
            regen.validate_body(b"".join(lines[:-1]))
        for index, line in enumerate(lines):
            if line.startswith(b"base64url\tembedded-nul\t"):
                lines[index] = line.replace(b"4d616e\n", b"4d616g\n")
                break
        else:
            self.fail("missing embedded-NUL row")
        with self.assertRaises(regen.ContentError):
            regen.validate_body(b"".join(lines))

    def test_pinned_range_rejects_shortened_and_extended_ends(self):
        upstream = ROOT.parent / "unified-wifi-mesh"
        entry = next(
            item for item in regen.RANGES if item[0] == regen.EM and item[1] == 828
        )
        source = regen.run(
            [
                "git",
                "--no-replace-objects",
                "-C",
                str(upstream),
                "show",
                f"{regen.PIN}:{regen.EM}",
            ]
        )
        self.assertTrue(regen.extract_range(source, entry).rstrip().endswith("}"))
        for last in (871, 873, 882):
            with self.subTest(last=last), self.assertRaises(regen.ContentError):
                regen.extract_range(source, (entry[0], entry[1], last, *entry[3:]))

    def test_validation_rejects_well_formed_invalid_results(self):
        fixture = regen.FIXTURE.read_bytes()
        body = fixture[fixture.index(b"# OpenSSL: ") :]
        regen.validate_body(body)
        dh_row = next(
            line
            for line in body.decode("ascii").splitlines()
            if line.startswith("dh\tp-minus-2\t")
        )
        dh_fields = dict(
            field.split("=", 1) for field in dh_row.split("\t")[4].split(";")
        )
        changes = {
            ("dh", "p-minus-2"): (
                "1",
                "upstream=-;rust=accept;rust_secret=" + dh_fields["rust_secret"],
            ),
            ("dh", "two"): ("1", "upstream=08;rust=reject;rust_secret=-"),
            ("signature", "p256"): ("1", "00"),
            ("wsc", "keys"): ("1", "-"),
            ("wsc", "kdk"): ("1", "00" * 32),
            ("cbc", "malformed-padding"): ("0", "span=-;rust=accept;rust_plain=-"),
            ("spki", "p192"): ("1", "upstream=00;rust=accept"),
            ("kdf80211", "sha256-8192"): ("1", "upstream=00;rust=accept"),
        }
        for (category, case), (ret, output) in changes.items():
            with self.subTest(case=f"{category}/{case}"):
                lines = body.decode("ascii").splitlines()
                for index, line in enumerate(lines):
                    cols = line.split("\t")
                    if cols[:2] == [category, case]:
                        cols[3:] = [ret, output]
                        lines[index] = "\t".join(cols)
                        break
                else:
                    self.fail("missing row")
                with self.assertRaisesRegex(regen.ContentError, f"{category}/{case}"):
                    regen.validate_body(("\n".join(lines) + "\n").encode("ascii"))

    def test_validation_rejects_siv_shape_rows_with_the_wrong_shape(self):
        fixture = regen.FIXTURE.read_bytes()
        body = fixture[fixture.index(b"# OpenSSL: ") :].decode("ascii")
        lines = body.splitlines()
        # Case name -> (text to find in the inputs, replacement).
        changes = {
            "encrypt-256-ad0-p15": ("ad=none", "ad=-"),
            "encrypt-256-adempty-p16": ("ad=-", "ad=none"),
            "encrypt-256-ad1-p32": ("ad=", "ad=00,"),
            "encrypt-256-ad1-p16": (";plain=", ";plain=00"),
        }
        for case, (old, new) in changes.items():
            with self.subTest(case=case):
                changed = []
                for line in lines:
                    cols = line.split("\t")
                    if cols[:2] == ["siv", case]:
                        self.assertIn(old, cols[2])
                        cols[2] = cols[2].replace(old, new, 1)
                    changed.append("\t".join(cols))
                with self.assertRaisesRegex(regen.ContentError, "siv/" + case):
                    regen.validate_body(("\n".join(changed) + "\n").encode("ascii"))

    def test_validation_rejects_rows_carrying_another_case(self):
        fixture = regen.FIXTURE.read_bytes()
        body = fixture[fixture.index(b"# OpenSSL: ") :].decode("ascii")
        regen.validate_body(body.encode("ascii"))
        rows = {
            tuple(line.split("\t")[:2]): line.split("\t")
            for line in body.splitlines()
            if line and not line.startswith("#")
        }
        swaps = {
            ("cbc", "aligned"): ("cbc", "empty"),
            ("cbc", "non-aligned"): ("cbc", "aligned"),
            ("dh", "leading-zero-exp-8"): ("dh", "two"),
            ("dh", "leading-zero-exp-17"): ("dh", "leading-zero-exp-8"),
        }
        for target, source in swaps.items():
            with self.subTest(target="/".join(target), source="/".join(source)):
                lines = []
                for line in body.splitlines():
                    cols = line.split("\t")
                    if tuple(cols[:2]) == target:
                        cols = list(target) + rows[source][2:]
                    lines.append("\t".join(cols))
                with self.assertRaisesRegex(regen.ContentError, "/".join(target)):
                    regen.validate_body(("\n".join(lines) + "\n").encode("ascii"))


class HarnessTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.tmp.cleanup)
        cls.scratch = Path(cls.tmp.name)
        sources = regen.read_sources(ROOT.parent / "unified-wifi-mesh")
        regen.write_extractions(sources, cls.scratch)
        regen.compile_harness(cls.scratch, "cc", "c++")
        cls.source = (cls.scratch / "vectors.cpp").read_text()

    def compile_variant(self, source):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        scratch = Path(tmp.name)
        for path in self.scratch.iterdir():
            if path.is_file() and path.name != "vectors":
                shutil.copy2(path, scratch / path.name)
        (scratch / "vectors.cpp").write_text(source)
        compiled = subprocess.run(
            [
                "c++",
                "-std=c++17",
                "-O2",
                "-Wno-deprecated-declarations",
                "vectors.cpp",
                "aes_siv.o",
                "-lcrypto",
                "-o",
                "vectors",
            ],
            cwd=scratch,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        return scratch, compiled

    def test_dh_rust_outcome_survives_upstream_rejection(self):
        needle = "auto got = dh(remote, priv);"
        self.assertEqual(self.source.count(needle), 1)
        source = self.source.replace(
            needle,
            'auto got = name == "p-minus-2" ? std::make_pair(0, Bytes{}) : dh(remote, priv);',
        )
        scratch, compiled = self.compile_variant(source)
        self.assertEqual(compiled.returncode, 0, compiled.stderr)
        output = scratch / "body.tsv"
        run = subprocess.run(
            [str(scratch / "vectors"), str(output)],
            cwd=scratch,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.assertEqual(run.returncode, 0, run.stderr)
        body = output.read_bytes()
        rows = [
            line.split("\t")
            for line in body.decode("ascii").splitlines()
            if not line.startswith("#")
        ]
        dh = {row[1]: row for row in rows if row[0] == "dh"}
        p = int(
            dict(field.split("=", 1) for field in dh["p"][2].split(";"))["peer"], 16
        )
        for name in ("zero", "one", "p-minus-1", "p", "p-plus-1"):
            fields = dict(field.split("=", 1) for field in dh[name][4].split(";"))
            self.assertEqual(
                (fields["rust"], fields["rust_secret"]), ("reject", "-"), name
            )
        fields = dict(field.split("=", 1) for field in dh["p-minus-2"][4].split(";"))
        self.assertEqual(
            (dh["p-minus-2"][3], fields["upstream"], fields["rust"]),
            ("0", "-", "accept"),
        )
        self.assertEqual(fields["rust_secret"], (p - 8).to_bytes(192, "big").hex())
        regen.validate_body(body)

    def test_siv_associated_data_types_are_checked_at_compile_time(self):
        cast = "reinterpret_cast<char *>(ad1.data())"
        self.assertEqual(self.source.count(cast), 3)
        source = self.source.replace(cast, "ad1.data()")
        _, compiled = self.compile_variant(source)
        self.assertNotEqual(
            compiled.returncode,
            0,
            "unsigned-char AD compiled through the typed adapter",
        )

    def test_siv_shape_rows_use_the_typed_adapters(self):
        cast = "reinterpret_cast<char *>(piece.data())"
        self.assertEqual(self.source.count(cast), 1)
        _, compiled = self.compile_variant(self.source.replace(cast, "piece.data()"))
        self.assertNotEqual(compiled.returncode, 0, "unsigned-char piece compiled")

    def test_harness_checks_verifier_arity_and_spki_limit(self):
        executable = str(self.scratch / "vectors")
        missing = subprocess.run(
            [executable, "--verify-signature"], capture_output=True, text=True
        )
        self.assertEqual(missing.returncode, 2, missing.stderr)
        self.assertIn("invalid arguments", missing.stderr)
        too_large = subprocess.run(
            [executable, "--verify-signature", "00", "00" * 1024, "00"],
            capture_output=True,
            text=True,
        )
        self.assertEqual(too_large.returncode, 2, too_large.stderr)
        self.assertIn("SPKI DER exceeds", too_large.stderr)
        at_limit = subprocess.run(
            [executable, "--verify-signature", "00", "00" * 1023, "00"],
            capture_output=True,
            text=True,
        )
        self.assertEqual(at_limit.returncode, 2, at_limit.stderr)
        self.assertIn("cannot import verification key", at_limit.stderr)

    def test_harness_checks_records_arity_and_file(self):
        executable = str(self.scratch / "vectors")
        missing_argument = subprocess.run(
            [executable, "--verify-records"], capture_output=True, text=True
        )
        self.assertEqual(missing_argument.returncode, 2, missing_argument.stderr)
        self.assertIn("invalid arguments", missing_argument.stderr)
        missing_file = subprocess.run(
            [executable, "--verify-records", "/nonexistent/records.tsv"],
            capture_output=True,
            text=True,
        )
        self.assertEqual(missing_file.returncode, 2, missing_file.stderr)
        self.assertIn("cannot open records file", missing_file.stderr)

    def test_harness_frees_owned_keys_on_exception(self):
        # Instrument only a temporary harness; force errors after EC_KEY and
        # X509_PUBKEY allocation, then count their frees on the exception paths.
        source = self.source.replace("EC_KEY_new_by_curve_name(", "tracked_ec_new(")
        source = source.replace("EC_KEY_free", "tracked_ec_free")
        source = source.replace("d2i_X509_PUBKEY(", "tracked_spki_new(")
        source = source.replace("X509_PUBKEY_free", "tracked_spki_free")
        tracking = """
static int ec_created = 0, ec_freed = 0, spki_created = 0, spki_freed = 0;
static EC_KEY *tracked_ec_new(int nid) {
    EC_KEY *key = EC_KEY_new_by_curve_name(nid);
    if (key) ++ec_created;
    return key;
}
static void tracked_ec_free(EC_KEY *key) { if (key) ++ec_freed; EC_KEY_free(key); }
static X509_PUBKEY *tracked_spki_new(X509_PUBKEY **key, const unsigned char **data, long len) {
    X509_PUBKEY *result = d2i_X509_PUBKEY(key, data, len);
    if (result) ++spki_created;
    return result;
}
static void tracked_spki_free(X509_PUBKEY *key) { if (key) ++spki_freed; X509_PUBKEY_free(key); }
"""
        source = source.replace(
            '#include "definitions.inc"', '#include "definitions.inc"\n' + tracking
        )
        marker = '    require(ec != nullptr, "EC_KEY_new failed");'
        self.assertEqual(source.count(marker), 1)
        source = source.replace(
            marker,
            marker + '\n    throw std::runtime_error("injected EC key failure");',
        )
        spki_line = next(
            line
            for line in regen.FIXTURE.read_text().splitlines()
            if line.startswith("spki\tp256\t")
        )
        der = dict(
            field.split("=", 1) for field in spki_line.split("\t")[2].split(";")
        )["der"]
        test_branch = (
            '''if (std::string(argv[1]) == "--resource-test") {
            try { fixed_key(curves[0]); } catch (const std::exception &) {}
            require(ec_created == 1 && ec_freed == 1, "EC_KEY leak");
            Bytes der = unhex("'''
            + der
            + """");
            der.push_back(0);
            try { spki_point(der); } catch (const std::exception &) {}
            require(spki_created == 1 && spki_freed == 1, "X509_PUBKEY leak");
        } else if (std::string(argv[1]) == "--verify-signature")"""
        )
        marker = 'if (std::string(argv[1]) == "--verify-signature")'
        self.assertEqual(source.count(marker), 1)
        source = source.replace(marker, test_branch)
        scratch, compiled = self.compile_variant(source)
        self.assertEqual(compiled.returncode, 0, compiled.stderr)
        run = subprocess.run(
            [str(scratch / "vectors"), "--resource-test"],
            capture_output=True,
            text=True,
        )
        self.assertEqual(run.returncode, 0, run.stderr)


class DriverTests(unittest.TestCase):
    def invoke(self, *args):
        return subprocess.run(
            [sys.executable, str(DRIVER), *map(str, args)],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

    def test_missing_pin_object_is_exit_2(self):
        with mock.patch.object(
            regen, "PIN", "0000000000000000000000000000000000000000"
        ), contextlib.redirect_stderr(io.StringIO()) as err:
            code = regen.main([])
        self.assertEqual(code, 2)
        self.assertIn("command failed", err.getvalue())

    def test_compile_failure_is_exit_2(self):
        with tempfile.TemporaryDirectory() as tmp:
            compiler = Path(tmp) / "compiler"
            compiler.write_text(
                f"#!{sys.executable}\nimport sys\n"
                "if '--version' in sys.argv: print('test compiler'); sys.exit(0)\n"
                "print('intentional compilation failure', file=sys.stderr); sys.exit(1)\n"
            )
            compiler.chmod(0o755)
            result = self.invoke("--cc", compiler)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("intentional compilation failure", result.stderr)
        self.assertIn("-c aes_siv.c", result.stderr)

    def test_check_mismatch_is_exit_1_and_does_not_write(self):
        with tempfile.TemporaryDirectory() as tmp:
            fixture = Path(tmp) / "fixture.tsv"
            generated = self.invoke("--out", fixture)
            self.assertEqual(generated.returncode, 0, generated.stderr)
            identical = self.invoke("--check", "--out", fixture)
            self.assertEqual(identical.returncode, 0, identical.stderr)
            changed = bytearray(fixture.read_bytes())
            vector_start = changed.index(b"\npbkdf2\t")
            output_start = changed.index(b"\t1\t", vector_start) + len(b"\t1\t")
            changed[output_start] = (
                ord("0") if changed[output_start] != ord("0") else ord("1")
            )
            fixture.write_bytes(changed)
            result = self.invoke("--check", "--out", fixture)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("fixture differs", result.stderr)
            self.assertEqual(fixture.read_bytes(), changed)

    def test_missing_check_fixture_is_io_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            fixture = Path(tmp) / "missing.tsv"
            with mock.patch.object(regen, "generate", return_value=(b"fixture", [])):
                with contextlib.redirect_stderr(io.StringIO()) as err:
                    code = regen.main(["--check", "--out", str(fixture)])
            self.assertEqual(code, 2)
            self.assertIn(str(fixture), err.getvalue())

    def test_validation_failure_does_not_overwrite_fixture(self):
        with tempfile.TemporaryDirectory() as tmp:
            fixture = Path(tmp) / "fixture.tsv"
            fixture.write_bytes(b"existing fixture\n")
            with mock.patch.object(
                regen,
                "validate_body",
                side_effect=regen.ContentError("invalid harness output"),
            ), contextlib.redirect_stderr(io.StringIO()) as err:
                code = regen.main(["--out", str(fixture)])
            self.assertEqual(code, 1)
            self.assertIn("invalid harness output", err.getvalue())
            self.assertEqual(fixture.read_bytes(), b"existing fixture\n")

    def test_invalid_harness_result_is_rejected_before_write(self):
        fixture = regen.FIXTURE.read_bytes()
        body = fixture[fixture.index(b"# OpenSSL: ") :]
        lines = body.decode("ascii").splitlines()
        for index, line in enumerate(lines):
            cols = line.split("\t")
            if cols[:2] == ["wsc", "keys"]:
                cols[-1] = "-"
                lines[index] = "\t".join(cols)
                break
        else:
            self.fail("WSC keys row missing")
        invalid = ("\n".join(lines) + "\n").encode("ascii")
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "fixture.tsv"
            target.write_bytes(b"existing fixture\n")

            def fake_run(command, **kwargs):
                Path(command[1]).write_bytes(invalid)
                return b""

            with (
                mock.patch.object(regen, "read_sources", return_value={}),
                mock.patch.object(regen, "write_extractions"),
                mock.patch.object(
                    regen, "compile_harness", return_value=("cc", "cxx", [], [], {})
                ),
                mock.patch.object(regen, "run", side_effect=fake_run),
                contextlib.redirect_stderr(io.StringIO()) as err,
            ):
                code = regen.main(["--out", str(target)])
            self.assertEqual(code, 1)
            self.assertIn("wsc/keys", err.getvalue())
            self.assertEqual(target.read_bytes(), b"existing fixture\n")

    def test_atomic_write_reports_cleanup_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "fixture.tsv"
            with (
                mock.patch.object(
                    regen.os, "replace", side_effect=PermissionError("replace denied")
                ),
                mock.patch.object(
                    regen.os, "unlink", side_effect=PermissionError("unlink denied")
                ),
            ):
                try:
                    regen.write_atomically(target, b"new fixture\n")
                except (regen.ToolError, OSError) as err:
                    failure = err
                else:
                    self.fail("atomic write unexpectedly succeeded")
            self.assertIsInstance(failure, regen.ToolError)
            message = str(failure)
            self.assertIn("replace denied", message)
            self.assertIn("unlink denied", message)
            leftovers = list(Path(tmp).glob(".tmp-*"))
            self.assertEqual(len(leftovers), 1)
            self.assertIn(str(leftovers[0]), message)

    def test_generated_form_is_separate_from_fixture(self):
        with tempfile.TemporaryDirectory() as tmp:
            fixture = Path(tmp) / "fixture.tsv"
            fixture.write_bytes(b"unchanged\n")
            result = self.invoke("--generated-form", "--out", fixture)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(fixture.read_bytes(), b"unchanged\n")
        for curve in ("p256", "p384", "p521"):
            self.assertIn(
                curve + "\tprovider=default\tpoint-form=uncompressed\tder=",
                result.stdout,
            )

    def test_verifier_rejects_oversized_spki_as_usage_error(self):
        result = self.invoke("--verify-signature", "00", "00" * 1024, "00")
        self.assertEqual(result.returncode, 2)
        self.assertIn("exceeds", result.stderr)

    def test_verifier_accepts_external_key_and_signature(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            private, public, message, signature = (
                tmp / name
                for name in ("key.pem", "public.der", "message", "signature.der")
            )
            message.write_bytes(b"external signing key")
            for command in (
                [
                    "openssl",
                    "ecparam",
                    "-name",
                    "prime256v1",
                    "-genkey",
                    "-noout",
                    "-out",
                    str(private),
                ],
                [
                    "openssl",
                    "pkey",
                    "-in",
                    str(private),
                    "-pubout",
                    "-outform",
                    "DER",
                    "-out",
                    str(public),
                ],
                [
                    "openssl",
                    "dgst",
                    "-sha256",
                    "-sign",
                    str(private),
                    "-out",
                    str(signature),
                    str(message),
                ],
            ):
                subprocess.run(
                    command, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE
                )
            result = self.invoke(
                "--verify-signature",
                message.read_bytes().hex(),
                public.read_bytes().hex(),
                signature.read_bytes().hex(),
            )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "verified=1")

    def test_verifier_accepts_fixture_and_rejects_changes(self):
        for line in regen.FIXTURE.read_text().splitlines():
            if line.startswith("signature\tp256\t"):
                _, _, inputs, _, signature = line.split("\t")
                fields = dict(pair.split("=", 1) for pair in inputs.split(";"))
                break
        else:
            self.fail("P-256 connector signature missing")
        message, spki = fields["message"], fields["spki"]
        accepted = self.invoke("--verify-signature", message, spki, signature)
        self.assertEqual(accepted.returncode, 0, accepted.stderr)
        self.assertEqual(accepted.stdout.strip(), "verified=1")
        changed_message = ("00" if message[:2] != "00" else "01") + message[2:]
        changed_signature = signature[:-2] + ("00" if signature[-2:] != "00" else "01")
        for changed_msg, changed_sig in (
            (changed_message, signature),
            (message, changed_signature),
        ):
            rejected = self.invoke("--verify-signature", changed_msg, spki, changed_sig)
            self.assertEqual(rejected.returncode, 1, rejected.stderr)
            self.assertEqual(rejected.stdout.strip(), "verified=0")

    def fixture_records(self):
        """`curve, spki, message, signature` for each fixed connector signature row."""
        records = []
        for line in regen.FIXTURE.read_text().splitlines():
            if line.startswith("signature\t"):
                _, curve, inputs, _, signature = line.split("\t")
                fields = dict(pair.split("=", 1) for pair in inputs.split(";"))
                records.append([curve, fields["spki"], fields["message"], signature])
        self.assertEqual([record[0] for record in records], ["p256", "p384", "p521"])
        return records

    def verify_records(self, text):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "records.tsv"
            path.write_text(text)
            return self.invoke("--verify-records", path)

    @staticmethod
    def records_text(records):
        return "".join("\t".join(record) + "\n" for record in records)

    def test_verify_records_accepts_valid_signatures_on_all_curves(self):
        result = self.verify_records(self.records_text(self.fixture_records()))
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = result.stdout.splitlines()
        self.assertEqual(lines[-1], "records=3 failures=0")
        for curve, line in zip(("p256", "p384", "p521"), lines):
            self.assertIn(
                f"curve={curve} accepted=1 rejects-changed-message=1 "
                "rejects-changed-signature=1",
                line,
            )

    def test_verify_records_reports_changed_message_and_signature(self):
        valid = self.fixture_records()[0]
        wrong_message = [valid[0], valid[1], "00" + valid[2][2:], valid[3]]
        wrong_signature = [valid[0], valid[1], valid[2], valid[3][:-2] + "00"]
        result = self.verify_records(
            self.records_text([valid, wrong_message, wrong_signature])
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        lines = result.stdout.splitlines()
        self.assertEqual(lines[-1], "records=3 failures=2")
        self.assertIn("accepted=1", lines[0])
        self.assertIn("accepted=0", lines[1])
        self.assertIn("accepted=0", lines[2])
        self.assertIn("signature record verification failed", result.stderr)

    def test_verify_records_rejects_malformed_files(self):
        valid = self.fixture_records()[0]
        malformed = {
            "empty file": "",
            "too few fields": "\t".join(valid[:3]) + "\n",
            "too many fields": "\t".join(valid + ["00"]) + "\n",
            "unknown curve": "\t".join(["p192"] + valid[1:]) + "\n",
            "upper-case hex": "\t".join(valid[:3] + [valid[3].upper()]) + "\n",
            "odd-length hex": "\t".join(valid[:3] + [valid[3][:-1]]) + "\n",
            "empty field": "\t".join(valid[:3] + [""]) + "\n",
            "blank line": self.records_text([valid]) + "\n",
            "oversized SPKI": "\t".join([valid[0], "00" * 1024, *valid[2:]]) + "\n",
        }
        for name, text in malformed.items():
            with self.subTest(name):
                result = self.verify_records(text)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertNotIn("verified", result.stdout)
                self.assertIn("error:", result.stderr)

    def test_verify_records_rejects_non_ascii_files_without_a_traceback(self):
        valid = self.records_text(self.fixture_records()[:1])
        undecodable = {
            "utf-8 text": valid.replace("p256", "p256\u00e9").encode("utf-8"),
            "invalid utf-8": b"p256\t\xff\xfe\n",
        }
        for name, data in undecodable.items():
            with self.subTest(name), tempfile.TemporaryDirectory() as tmp:
                path = Path(tmp) / "records.tsv"
                path.write_bytes(data)
                result = self.invoke("--verify-records", path)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertNotIn("Traceback", result.stderr)
                self.assertIn("error:", result.stderr)
                self.assertIn("not ASCII text", result.stderr)

    def test_verify_records_errors_from_the_harness_are_exit_2(self):
        valid = self.fixture_records()
        # A P-384 key labelled P-256, and an SPKI that is not a key at all.
        mislabelled = [valid[0][0], *valid[1][1:]]
        not_a_key = [valid[0][0], "00", *valid[0][2:]]
        for name, record in (("mislabelled curve", mislabelled), ("not a key", not_a_key)):
            with self.subTest(name):
                result = self.verify_records(self.records_text([record]))
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn("oracle error", result.stderr)

    def test_verify_records_missing_file_is_exit_2(self):
        result = self.invoke("--verify-records", ROOT / "does-not-exist.tsv")
        self.assertEqual(result.returncode, 2)
        self.assertIn("error:", result.stderr)


if __name__ == "__main__":
    unittest.main()
