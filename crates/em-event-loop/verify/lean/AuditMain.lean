import AuditAxioms
import StepSpec
import Rules
import Bridge
import Reference
import Examples
import Extracted.Types
import Extracted.Funs
import Abstraction
import Refinement
import Transfer
import ExtractedRun
import ExtractedExamples
import CounterExamples

/-! The gate for every project module except `PlantedBugs` (which cannot share an environment
with `Examples`: both define `EmEventLoop.Loop`; see `AuditPlanted.lean`). The number after each
module is the checked-in count of its non-internal theorems. `native_decide` is allowed in
`ExtractedExamples` only. -/

#eval EmEventLoopAudit.run
  [(`StepSpec, some 45), (`Rules, some 91), (`Bridge, some 6), (`Reference, some 30), (`Examples, some 17),
   (`Extracted.Types, some 23), (`Extracted.Funs, some 0), (`Abstraction, some 0), (`Refinement, some 44),
   (`Transfer, some 26), (`ExtractedRun, some 17), (`ExtractedExamples, some 27), (`CounterExamples, some 29)]
  [`ExtractedExamples]
