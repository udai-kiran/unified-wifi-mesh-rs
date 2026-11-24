import Lean
import Transfer

/-! # The refinement contract, printed from the elaborated types (note 0017 G5, R7)

Prints the statements that say what the extracted code must do: the types of the nine `*_refines`
theorems, the four `*_fails` theorems, `RustChange`, `RustPassTrace`, `RustPassTrace_transfer`
and every `extracted_S*` theorem of `Transfer.lean`. The text comes from `ConstantInfo.type` (and
the constructors of an inductive type or structure), not from the source file, so a change of a
statement changes the output and a change of a proof does not. `refinement-digest.sh` hashes this
text together with `Abstraction.lean`.

With the environment variable `EM_STATEMENTS_OUT` set, the text is also written to that file. -/

open Lean Meta Elab Command

namespace EmEventLoop

def refinementNames : List Name :=
  [`set_ready_refines, `set_paused_refines, `set_deadline_refines, `set_deadline_paused_refines,
   `begin_pass_refines, `new_refines, `has_ready_source_refines, `next_deadline_refines,
   `next_turn_refines, `set_ready_fails, `set_paused_fails, `set_deadline_fails,
   `set_deadline_paused_fails, `RustChange, `RustPassTrace, `RustPassTrace_transfer].map
    (`EmEventLoop ++ ·)

/-- The `extracted_S*` theorems of `Transfer.lean`, found by name in the environment. -/
def extractedRuleNames (env : Environment) : List Name :=
  let isExtractedRule (constName : Name) : Bool :=
    constName.getPrefix == `EmEventLoop && constName.getString!.startsWith "extracted_S" &&
      env.getModuleIdxFor? constName == env.getModuleIdx? `Transfer
  (env.constants.map₁.toList.map (·.1)).filter isExtractedRule

def typeLine (constName : Name) (type : Expr) : MetaM String := do
  let options : Options := Options.empty
    |>.setBool `pp.fullNames true |>.setBool `pp.funBinderTypes true
    |>.setBool `pp.universes true
  let shown ← withOptions (fun _ => options) (ppExpr type)
  return s!"{constName} : {shown.pretty 1000000}"

def statementLines (constName : Name) : MetaM (List String) := do
  let env ← getEnv
  let some info := env.find? constName | throwError "frozen name {constName} does not exist"
  let mut lines := [← typeLine constName info.type]
  if let .inductInfo induct := info then
    for ctor in induct.ctors do
      let some ctorInfo := env.find? ctor | throwError "constructor {ctor} does not exist"
      lines := lines ++ [← typeLine ctor ctorInfo.type]
  return lines

#eval show CommandElabM Unit from do
  let env ← getEnv
  let extractedNames := (extractedRuleNames env).toArray.qsort Name.lt
  let names := refinementNames ++ extractedNames.toList
  let mut lines : List String := []
  for constName in names do
    lines := lines ++ (← liftTermElabM (statementLines constName))
  let text := "\n".intercalate lines ++ "\n"
  logInfo m!"{lines.length} statement lines for {names.length} names"
  if let some path ← IO.getEnv "EM_STATEMENTS_OUT" then
    IO.FS.writeFile path text

end EmEventLoop
