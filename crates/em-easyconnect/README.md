# em-easyconnect

Wi-Fi Easy Connect (DPP) onboarding boundary for the EasyMesh port. For S1,
`onboarding_in_progress(al_mac)` always returns false. It represents the
onboarding query in `handle_state_config_none` at
`src/em/config/em_configuration.cpp:6695-6710`; the DPP state machines and
1905 security layer are later slices (note [0012](../../docs/0012-s1-discovery.md)
D1 and D10).

## Open points

There is no per-AL onboarding state in S1. A later slice must replace this
constant boundary with the Easy Connect enrollee's actual state.

## Upstream source

- `src/em/prov/easyconnect/*.cpp`
- `inc/ec_*.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.
