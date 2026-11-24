import Lean

/-! # The checker of the verification gates (note 0017 G4, step 3b)

Lean decides, from the elaborated environment, whether the proofs of the project hold. No source
text is parsed. For the constants of the listed project modules the checker:

* collects the axioms of every theorem, definition and opaque constant (`Lean.collectAxioms`) and
  allows only `propext`, `Classical.choice`, `Quot.sound`;
* fails on `sorryAx` (a `sorry`, an `admit`, or a theorem whose proof failed);
* fails on every `axiom` declared in a project module;
* allows the axiom `<decl>._native.native_decide.ax_*` that `native_decide` creates only when
  `<decl>` is a theorem of a module in the `nativeModules` list (the examples that run the
  extracted code, and the counter-example of a planted bug);
* compares the number of non-internal theorems of each module with a checked-in number, so a
  lost theorem fails.

`example` never enters the environment, so the checked files state their checks as named theorems.

`EmEventLoopAudit.run` is the gate for the modules the current file imports. `writeVerdict` audits
the file being compiled and writes the result to a marker file (used for the planted bugs, whose
refinement proof is expected to fail). A missing marker file means the file did not compile to
its end. -/

open Lean Elab Command

namespace EmEventLoopAudit

/-- The module of the file being compiled, for constants that are not imported. -/
def thisModule : Name := `_this

def standardAxioms : List Name := [`propext, `Classical.choice, `Quot.sound]

structure Violation where
  code : String
  name : Name
  detail : String

structure Report where
  violations : Array Violation := #[]
  /-- Non-internal theorems of the audited modules with the axioms they depend on. -/
  theorems : Array (Name × Array Name) := #[]
  /-- Number of non-internal theorems per module. -/
  counts : List (Name × Nat) := []

def Report.add (report : Report) (code : String) (name : Name) (detail : String) : Report :=
  { report with violations := report.violations.push { code, name, detail } }

def ownerOf (env : Environment) (n : Name) : Name :=
  match env.getModuleIdxFor? n with
  | some idx => env.header.moduleNames[idx.toNat]!
  | none => thisModule

/-- The theorem `decl` for an axiom named `decl._native.native_decide.ax_*`. -/
def nativeDeclOf? : Name → Option Name
  | .str (.str (.str decl "_native") "native_decide") last =>
      if last.startsWith "ax_" then some decl else none
  | _ => none

/-- A `native_decide` axiom of a theorem declared in one of the modules where it is allowed. -/
def isAllowedNative (env : Environment) (nativeModules : List Name) (axiomName : Name) : Bool :=
  match nativeDeclOf? axiomName with
  | none => false
  | some decl =>
      nativeModules.contains (ownerOf env axiomName) &&
        ownerOf env decl == ownerOf env axiomName &&
        (match env.find? decl with
          | some (.thmInfo _) => true
          | _ => false)

def audit (modules : List (Name × Option Nat)) (nativeModules : List Name) :
    CommandElabM Report := do
  let env ← getEnv
  let moduleNames := modules.map (·.1)
  let allConstants := env.constants.map₁.toList ++ env.constants.map₂.toList
  let mut report : Report := {}
  let mut counts : Std.HashMap Name Nat := {}
  for (name, info) in allConstants do
    let owner := ownerOf env name
    unless moduleNames.contains owner do continue
    match info with
    | .axiomInfo _ =>
        if nativeDeclOf? name |>.isSome then
          unless isAllowedNative env nativeModules name do
            report := report.add "native-outside" name s!"native_decide axiom in module {owner}"
        else
          report := report.add "axiom-user" name s!"axiom declared in module {owner}"
    | .thmInfo _ | .defnInfo _ | .opaqueInfo _ =>
        let isTheorem := info matches .thmInfo _
        let isInternal := name.isInternalDetail
        if isTheorem && !isInternal then
          counts := counts.insert owner (counts.getD owner 0 + 1)
        unless isInternal do
          let used ← collectAxioms name
          for axiomName in used do
            if standardAxioms.contains axiomName then continue
            if isAllowedNative env nativeModules axiomName then continue
            let code := if axiomName == `sorryAx then "sorryAx" else "axiom-forbidden"
            report := report.add code name s!"depends on {axiomName}"
          if isTheorem then
            report := { report with theorems := report.theorems.push (name, used) }
    | _ => pure ()
  let mut moduleCounts : List (Name × Nat) := []
  for (moduleName, expected) in modules do
    let found := counts.getD moduleName 0
    moduleCounts := moduleCounts ++ [(moduleName, found)]
    if let some expectedCount := expected then
      if found != expectedCount then
        report := report.add "count" moduleName s!"{found} theorems, expected {expectedCount}"
  return { report with counts := moduleCounts }

def violationLine (v : Violation) : String :=
  s!"AUDIT[{v.code}] {v.name}: {v.detail}"

/-- The gate for imported modules: the Lean error (and exit code 1) is the verdict. -/
def run (modules : List (Name × Option Nat)) (nativeModules : List Name) : CommandElabM Unit := do
  let report ← audit modules nativeModules
  for (moduleName, found) in report.counts do
    logInfo m!"AUDIT module {moduleName}: {found} theorems"
  if report.violations.isEmpty then
    logInfo m!"AUDIT ok: {report.counts.length} modules, {report.theorems.size} theorems"
  else
    let lines := report.violations.toList.map violationLine
    throwError "{"\n".intercalate lines}\nAUDIT FAILED: {report.violations.size} violations"

/-- The audit of the file being compiled, written to `path`; the last line `done` shows that the
file was compiled to its end. -/
def writeVerdict (path : String) (nativeModules : List Name) : CommandElabM Unit := do
  let report ← audit [(thisModule, none)] nativeModules
  let theoremLines := report.theorems.toList.map fun (name, used) =>
    let axioms := ",".intercalate (used.toList.map toString)
    s!"thm {name} {if axioms.isEmpty then "-" else axioms}"
  let violationLines := report.violations.toList.map fun v =>
    s!"viol {v.code} {v.name} {v.detail}"
  IO.FS.writeFile path ("\n".intercalate (theoremLines ++ violationLines ++ ["done"]) ++ "\n")

end EmEventLoopAudit
