# Pinned upstream TLV vectors (senders and readers)

`regen` builds a temporary executable from the exact pinned upstream git objects, a declaration
shim and the fixed inputs in `vectors.cpp`, runs it, and writes two fixtures:
`crates/em-msg/tests/data/upstream_tlv_vectors.tsv` (the senders, note 0009 D7 item 2) and
`crates/em-msg/tests/data/upstream_reader_vectors.tsv` (the `em_msg_t` readers, note 0009 D6 and
A10). The upstream checkout
defaults to the sibling `unified-wifi-mesh` directory and is read only through
`git --no-replace-objects -C <upstream> show <pin>:<path>`, where `<pin>` is the full `ba4180b`
commit (`PIN` in `regen`); use `--upstream PATH` for another
checkout. Host prerequisites are Python 3, git and a C++17 compiler (`c++`, or `--cxx`). No
library is linked.

```sh
python3 tools/upstream-tlv-vectors/regen            # write the fixture atomically
python3 tools/upstream-tlv-vectors/regen --check    # rebuild in memory, compare byte for byte
python3 -m unittest tools/test_upstream_tlv_vectors.py
```

Exit 0: success. Exit 1: an extraction failed its checks, the harness output failed validation,
or `--check` found a stale fixture. Exit 2: usage, I/O (a missing fixture under `--check` too), git
or compiler failure, or a missing, malformed, duplicate, or mismatched `sizeof`/`offsetof` probe.
Nothing is written unless the whole build validates.
`--check` compares both fixtures, including the compiler version in their headers, so a toolchain
change shows up as a diff to review.

## How extraction works

Every extracted definition is a line range with an expected first line.

- **Structs** (`STRUCTS` in `regen`) and the enums and small structs of `PLAIN_TYPES` go to
  `structs.inc`, which `shim.hpp` includes. The range must start with the expected line, have
  balanced braces (comments and literals masked), and end with `__attribute__((__packed__)) NAME;`.
  A `PLAIN_TYPES` entry (an enum, or a struct without the packed attribute) ends with `NAME;`. The
  shim holds no struct layout. Its hand-written
  definitions are the `mac_address_t`, `mac_addr_t` and `bssid_t` typedefs, with their OneWifi
  citations, the no-op `em_printfout` macro, and the `em_msg_t` declaration described under
  [Readers](#readers).
- **Senders** (`SENDERS`) go to `senders.inc`, which `vectors.cpp` includes. The range must start
  with the expected signature and its outermost closing brace must be the last token. The class
  qualifier (`em_t::`) is removed from the first line only, so the function compiles as a free
  function; the body is untouched.
- **Probe.** `vectors.cpp` prints one `# Probe:` line per struct with `sizeof` and `offsetof` of its
  whole-byte members (bit-fields have no `offsetof`). `PROBE_EXPECTED` in `regen` holds the values;
  a mismatch is exit 2. The probe lines are part of the sender fixture.

The sender fixture's columns are `case`, `function`, `inputs`, `return`, `hex`. `inputs` is
`name=value;...`, `hex` is lowercase with `-` for empty, and `return` is the function's return
value (the number of bytes the sender reports). The output buffer is pre-filled with `0xff`
(`fill=ff`), so bytes a sender does not write, or bits it does not clear, are visible. Each case's
input fields are checked against the fixed values in `expected_case_inputs()`.
The Profile-2 capability row must be exactly the sender's four assigned bytes `00 00 10 00`.

## Adding a sender

1. Find the function's line range at the pin and add one entry to `SENDERS`:
   `(path, first, last, "<exact first line>", "<Class>")`. If it needs a struct from
   `inc/em_base.h`, add an entry to `STRUCTS` and its expected sizes to `PROBE_EXPECTED`, and
   print the matching `# Probe:` line in `write_probe` in `vectors.cpp`.
2. Add a `write_<name>` function to `vectors.cpp` that calls the free function with fixed inputs and
   writes rows with `write_row`; call it from `main`.
3. Add each case and its exact input fields to `expected_case_inputs()` in `regen` (which also
   determines `expected_cases()` order), and add a branch to `check_row()` that checks the return
   value and byte relationships independently of the harness.
4. Run `regen`, review the diff of the fixture, run `regen --check`.

If the function needs another type (an `em_cmd_t`, a constant), extract it the same way. Do not
type its layout into the shim.

## Senders

`SENDERS` holds seven functions: `em_t::create_prof_2_tlv`, `em_configuration_t::create_error_code_tlv`,
`em_metrics_t::create_error_code_tlv`, `em_capability_t::create_client_info_tlv`,
`em_policy_cfg_t::create_backhaul_bss_conf_policy_tlv`,
`em_metrics_t::create_unassoc_sta_link_metrics_query_tlv` and
`em_metrics_t::create_unassoc_sta_link_metrics_resp_tlv`. `vectors.cpp` runs them on 31 fixed
cases (`expected_case_inputs()` in `regen` lists them), which is the sender fixture's row count.

## Readers

`READERS` holds the `em_msg_t` reader functions that note 0009 D6 and A10 port:
`get_al_mac_address`, `get_supported_service`, `parse_profile_tlv`, `get_profile`, `get_sta_mac`,
`get_bss_id`, `get_radio_id`, `get_freq_band`, `get_profile_type`, both `get_tlv` overloads, and
`get_first_tlv` and `get_next_tlv`. `get_tlv(em_tlv_t *)` and `get_client_mac_info` are not
extracted: they have no production caller, and the test map records them as not ported.

Each reader is extracted by line range, with its `em_msg_t::` qualifier kept, into `readers.inc`.
The shim declares `struct em_msg_t` with the two data members the readers use, `m_buff` and
`m_len`, and the method declarations of the pinned class. `regen` rejects any other member, and any
declaration that does not appear in `inc/em_msg.h` at the pin. A reader that needs another member
fails the build, so the shim is not widened by hand.

`vectors.cpp` fills each area with the upstream enumerators and runs the readers on 73 fixed TLV
areas:

- the whole-area readers and `get_first_tlv` on each area;
- `get_tlv` by type, for each area and each listed type, both overloads;
- `get_next_tlv` from the offset of each TLV in each area;
- `parse_profile_tlv` on its own cases, given as value and length.

The output objects are pre-filled with `0xff`, so bytes a reader does not write show up. The
reader fixture has 1421 rows, with the columns `case`, `function`, `inputs`, `area`, `return`
and `output`. `area` is the hex of the TLV area (for `parse_profile_tlv`, the value). `return` is
`true`, `false`, an offset into the area, or `-` for NULL. `output` is the hex of the whole output
object (`sizeof` of its type, pre-filled with `ff`), so an `ff` byte is one the reader did not
write. It is `-` for the walkers and `get_tlv`, which write no output object. `inputs` holds `len` (the area length), `from` (for `get_next_tlv`),
`type` and `value_len` where the function takes them.

`regen` does not trust the harness. `check_reader_row()` rebuilds each area from its own table
(`READER_AREAS`) and compares the bytes, and `model_reader()` computes the expected return and
output independently of `vectors.cpp`. A mistyped area, type or value fails the run.

The Rust side is `crates/em-msg/src/upstream_reader_vectors.rs` (test only). It maps each upstream
result onto the port's `Result<Option<T>, ReaderError>`. `crates/em-msg/tests/data/upstream_test_map.tsv`
names the Rust test for each reader and for each intentional difference, and
`tools/check-test-map` checks that those tests exist.
