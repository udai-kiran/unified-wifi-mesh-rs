import Lake
open Lake DSL

-- Mathlib comes from the Aeneas library built into the Docker volume (only the order
-- classes are used). The build output goes to the cache volume, so no .lake folder appears here.
require aeneas from "/opt/aeneas/backends/lean"

-- allowNonModules: the project files are not Lean `module`s; without it `lake build --wfail`
-- fails on the warning that says so.
package «em-event-loop-verify» where
  packagesDir := "/opt/aeneas/backends/lean/.lake/packages"
  buildDir := "/cache/lake-em-event-loop"
  allowNonModules := true

-- Every .lean file of this folder is a root (check-modules.py checks it). `lake build --wfail`
-- builds all of them, so the axiom audit (AuditMain, AuditPlanted) runs in every full build.
@[default_target] lean_lib «EmEventLoop» where
  roots := #[`StepSpec, `Rules, `Bridge, `Reference, `Examples, `Extracted.Types, `Extracted.Funs,
    `Abstraction, `Refinement, `Transfer, `ExtractedRun, `ExtractedExamples, `CounterExamples,
    `PlantedBugs, `AuditAxioms, `AuditMain, `AuditPlanted, `RefinementStatements]
