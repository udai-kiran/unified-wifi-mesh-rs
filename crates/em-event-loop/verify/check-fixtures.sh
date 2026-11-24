#!/usr/bin/env bash
# R3: the gate must FAIL, with the right reason, on each bad file. Run INSIDE the container,
# after RUN/proofs (a built copy of verify/lean) exists:
#   verify/run.sh bash /crate/verify/check-fixtures.sh RUN
# Each fixture in fixtures/ is appended to a copy of a real audited module (Bridge.lean) and the
# audit of AuditAxioms.lean is run on the copy, as the real gate does. Exit 0 only if every case
# behaves as expected.
set -uo pipefail
RUN=${1:-}
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
case "$RUN" in /cache/em-event-loop-runs/?*) ;; *) echo "bad run folder: '$RUN'" >&2; exit 2 ;; esac
case "$RUN" in *..*) echo "'..' in the run folder" >&2; exit 2 ;; esac
PROOFS=$RUN/proofs
OUT=$RUN/fixtures
[ -d "$PROOFS" ] || { echo "no built proofs copy: $PROOFS" >&2; exit 2; }
mkdir -p "$OUT"
status=0

# expect NAME EXIT_KIND REGEX: EXIT_KIND is `fail` (non-zero exit) or `pass` (exit 0)
expect() {
  local name=$1 kind=$2 regex=$3 rc=$4 log=$5
  if [ "$kind" = fail ] && [ "$rc" -eq 0 ]; then echo "FAIL $name: the gate passed (exit 0)"; status=1; return; fi
  if [ "$kind" = pass ] && [ "$rc" -ne 0 ]; then echo "FAIL $name: the gate failed (exit $rc)"; status=1; return; fi
  if grep -Eq "$regex" "$log"; then
    echo "ok   $name: exit $rc, $(grep -E "$regex" "$log" | head -1 | cut -c1-200)"
  else
    echo "FAIL $name: exit $rc but the output has no line matching $regex"; status=1
  fi
}

# audit_copy NAME FIXTURE NATIVE_MODULES: fixture appended to a copy of Bridge.lean, then audited
audit_copy() {
  local name=$1 fixture=$2 native=$3
  { echo "import AuditAxioms"; cat "$PROOFS/Bridge.lean"; echo
    cat "$HERE/fixtures/$fixture.lean"; echo
    echo "#eval EmEventLoopAudit.run [(\`_this, none)] [$native]"; } > "$OUT/$name.lean"
  (cd "$PROOFS" && lake env lean "$OUT/$name.lean") > "$OUT/$name.log" 2>&1
  echo $?
}

rc=$(audit_copy sorry sorry ""); expect sorry fail 'AUDIT\[sorryAx\] fixture_sorry' "$rc" "$OUT/sorry.log"
rc=$(audit_copy admit admit ""); expect admit fail 'AUDIT\[sorryAx\] fixture_admit' "$rc" "$OUT/admit.log"
rc=$(audit_copy axiom axiom ""); expect axiom fail 'AUDIT\[axiom-user\] fixture_axiom' "$rc" "$OUT/axiom.log"
expect axiom-use fail 'AUDIT\[axiom-forbidden\] fixture_uses_axiom: depends on fixture_axiom' "$rc" "$OUT/axiom.log"
rc=$(audit_copy native-outside native ""); expect native-outside fail 'AUDIT\[native-outside\] .*fixture_native' "$rc" "$OUT/native-outside.log"
# controls: a clean file passes, and native_decide passes where it is allowed
rc=$(audit_copy clean clean ""); expect clean pass 'AUDIT ok' "$rc" "$OUT/clean.log"
rc=$(audit_copy native-allowed native "\`_this"); expect native-allowed pass 'AUDIT ok' "$rc" "$OUT/native-allowed.log"

# Lean's own warning: `lake build --wfail` and -DwarningAsError turn sorry and admit into errors
for name in sorry admit; do
  (cd "$PROOFS" && lake env lean -DwarningAsError=true "$OUT/$name.lean") > "$OUT/$name-werror.log" 2>&1
  expect "$name-werror" fail "error: .*declaration uses .sorry." $? "$OUT/$name-werror.log"
done

# a lost theorem: the checked-in count is higher than the number of theorems in the module
{ echo "import AuditAxioms"; echo "import Bridge"
  echo "#eval EmEventLoopAudit.run [(\`Bridge, some 7)] []"; } > "$OUT/count.lean"
(cd "$PROOFS" && lake env lean "$OUT/count.lean") > "$OUT/count.log" 2>&1
expect lost-theorem fail 'AUDIT\[count\] Bridge: 6 theorems, expected 7' $? "$OUT/count.log"

# file layer (check-modules.py) on copies of the lean folder
module_case() {  # name kind regex
  local copy=$OUT/lean-$1
  mkdir -p "$copy"
  (cd "$HERE"/lean && tar --exclude=./.lake -cf - .) | tar -xf - -C "$copy"
  case "$1" in
    stray) echo "theorem stray : True := trivial" > "$copy/Stray.lean" ;;
    skipkernel) printf 'set_option debug.skipKernelTC true\n' >> "$copy/Bridge.lean" ;;
    implementedby) printf '@[implemented_by foo]\n' >> "$copy/Bridge.lean" ;;
  esac
  python3 "$HERE/check-modules.py" "$copy" > "$OUT/lean-$1.log" 2>&1
  expect "$1" fail "$2" $? "$OUT/lean-$1.log"
}
module_case stray 'Stray.lean is not a root of the lakefile'
module_case skipkernel 'Bridge.lean:[0-9]+: forbidden line: set_option debug.skipKernelTC'
module_case implementedby 'Bridge.lean:[0-9]+: forbidden line: @\[implemented_by'

[ "$status" -eq 0 ] && echo "ALL FIXTURES BEHAVE" || echo "SOME FIXTURE DID NOT BEHAVE"
exit $status
