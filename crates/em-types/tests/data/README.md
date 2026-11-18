# em-types golden test data

| File | Kind | Description |
|---|---|---|
| `upstream_enums.tsv` | generated | Every enumerator of the six D2 enums from upstream `inc/em_base.h` at the pinned commit, with the value upstream's C compiler assigns. Produced by `tools/gen-upstream-enums`; verify with `tools/gen-upstream-enums --check`. |
| `classification.tsv` | hand-maintained | One row per enumerator: class (`spec` / `vendor` / `internal` / `sentinel`), Rust name, spec reference, reason. Schema in [docs/0001-em-types-scope.md](../../../../docs/0001-em-types-scope.md) D4. |

Provenance of the committed `upstream_enums.tsv` (recorded by hand; not regenerated):

- Generated 2026-11-18 with: `cc (Ubuntu 15.2.0-16ubuntu1) 15.2.0`
- Upstream: `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f`, `inc/em_base.h` (Apache-2.0).

## Review status of `classification.tsv`

- **EasyMesh rows** were checked against the Wi-Fi EasyMesh Specification v6.0 text: message types
  (Table 21, §17.1), TLV types (Table 22 and Tables 126/127, §17.2), frequency band (Table 15),
  Multi-AP Profile (Table 69) and SupportedService (Table 23).
- **31 rows are provisional** (design note 0001, amendment A2): the 1905.1-only message types,
  TLVs and media types whose `spec_reference` reads `… (unverified …)`. Their values are fixed by
  the pinned upstream enums; their Rust names and references wait for a check against IEEE 1905.1.
- List them with:
  `awk -F'\t' 'NR>1 && $5 ~ /unverified/' classification.tsv`

## Bumping the pinned upstream commit

1. Change `PINNED_COMMIT` in `tools/gen-upstream-enums`, and the expected enumerator counts if
   upstream added or removed values.
2. Run `tools/gen-upstream-enums --no-classification` to regenerate `upstream_enums.tsv`.
3. Update `classification.tsv` so it covers the new fixture one-to-one.
4. Run `tools/gen-upstream-enums --check`, which must pass with the classification included.
5. Update the provenance lines above.
