import Lake
open Lake DSL

-- Mathlib comes from the Aeneas library built into the Docker volume (only the order
-- classes are used). The build output goes to the cache volume, so no .lake folder appears here.
require aeneas from "/opt/aeneas/backends/lean"

package «em-event-loop-verify» where
  packagesDir := "/opt/aeneas/backends/lean/.lake/packages"
  buildDir := "/cache/lake-em-event-loop"

@[default_target] lean_lib «EmEventLoop» where
  roots := #[`StepSpec, `Rules, `Bridge, `Reference, `Examples]
