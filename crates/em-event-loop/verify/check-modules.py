#!/usr/bin/env python3
"""Checks the file layer of the Lean gate (note 0017 step 3b, R2). Lean checks the proofs
(AuditAxioms.lean); this script checks that Lean was asked about every file.

  check-modules.py [LEAN_DIR]

Fails if
* a .lean file of LEAN_DIR (default: lean/) is not a root of the lakefile, or a root has no file;
* a project module is in neither driver of the axiom audit (AuditMain.lean, AuditPlanted.lean),
  or a driver names a module that is not a root;
* a line starts with `set_option debug.skipKernelTC` or `@[implemented_by` (they switch the kernel
  check off or replace a definition by other code).
"""
import pathlib
import re
import sys

lean_dir = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else pathlib.Path(__file__).parent / "lean"
TOOLING = {"AuditAxioms", "AuditMain", "AuditPlanted", "RefinementStatements"}
DRIVERS = ["AuditMain", "AuditPlanted"]
FORBIDDEN_LINE = re.compile(r"^\s*(set_option\s+debug\.skipKernelTC|@\[[^\]]*implemented_by)")

problems = []
files = {p.relative_to(lean_dir).with_suffix("").as_posix().replace("/", ".")
         for p in lean_dir.rglob("*.lean") if p.name != "lakefile.lean" and ".lake" not in p.parts}
lakefile = (lean_dir / "lakefile.lean").read_text()
roots_text = re.search(r"roots := #\[(.*?)\]", lakefile, re.S).group(1)
roots = set(re.findall(r"`([\w.]+)", roots_text))
for module in sorted(files - roots):
    problems.append(f"{module}.lean is not a root of the lakefile (Lean would never check it)")
for module in sorted(roots - files):
    problems.append(f"lakefile root {module} has no file")

audited = set()
for driver in DRIVERS:
    audited |= set(re.findall(r"\(`([\w.]+), some \d+\)", (lean_dir / f"{driver}.lean").read_text()))
for module in sorted((roots - TOOLING) - audited):
    problems.append(f"{module} is not in the axiom audit (AuditMain.lean or AuditPlanted.lean)")
for module in sorted(audited - roots):
    problems.append(f"the axiom audit names {module}, which is not a lakefile root")

for path in sorted(lean_dir.rglob("*.lean")):
    if ".lake" in path.parts:
        continue
    for number, line in enumerate(path.read_text().split("\n"), 1):
        if FORBIDDEN_LINE.match(line):
            problems.append(f"{path.relative_to(lean_dir)}:{number}: forbidden line: {line.strip()}")

for problem in problems:
    print("MODULES FAIL:", problem)
if not problems:
    print(f"modules ok: {len(roots)} roots = {len(files)} files; {len(audited)} audited, {len(TOOLING)} tooling")
sys.exit(1 if problems else 0)
