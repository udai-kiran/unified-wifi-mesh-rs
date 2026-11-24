#!/usr/bin/env python3
"""G5 (note 0017): digest of the frozen Lean statements. Proof bodies may change; statements may not."""
import hashlib, pathlib, re

here = pathlib.Path(__file__).parent
rules = (here / "Rules.lean").read_text()
parts = re.split(r"(?m)^(?=theorem )", rules)[1:]
signatures = [re.sub(r"\s+", " ", part.split(":= by", 1)[0]).strip() for part in parts]
defs = re.sub(r"\s+", " ", re.split(r"(?m)^theorem ", rules, maxsplit=1)[0]).strip()
blob = "\n".join([(here / "StepSpec.lean").read_text(), defs, *signatures])
print(len(signatures), hashlib.sha256(blob.encode()).hexdigest())
