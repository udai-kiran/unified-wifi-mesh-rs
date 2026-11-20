# Pinned upstream TLV sender vectors

`regen` builds a temporary executable from the exact pinned upstream git objects, a declaration
shim and the fixed inputs in `vectors.cpp`, runs it, and writes
`crates/em-msg/tests/data/upstream_tlv_vectors.tsv` (note 0009 D7 item 2). The upstream checkout
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
`--check` compares the whole file, including the compiler version in the header, so a toolchain
change shows up as a diff to review.

## How extraction works

Every extracted definition is a line range with an expected first line.

- **Structs** (`STRUCTS` in `regen`) go to `structs.inc`, which `shim.hpp` includes. The range must
  start with the expected line, have balanced braces (comments and literals masked), and end with
  `__attribute__((__packed__)) NAME;`. The shim holds no hand-written layout. Its only
  hand-written definitions are the `mac_address_t` and `mac_addr_t` typedefs, with their OneWifi
  citations.
- **Senders** (`SENDERS`) go to `senders.inc`, which `vectors.cpp` includes. The range must start
  with the expected signature and its outermost closing brace must be the last token. The class
  qualifier (`em_t::`) is removed from the first line only, so the function compiles as a free
  function; the body is untouched.
- **Probe.** `vectors.cpp` prints one `# Probe:` line per struct with `sizeof` and `offsetof` of its
  whole-byte members (bit-fields have no `offsetof`). `PROBE_EXPECTED` in `regen` holds the values;
  a mismatch is exit 2. The probe lines are part of the fixture.

The fixture columns are `case`, `function`, `inputs`, `return`, `hex`. `inputs` is
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

## Readers (not implemented yet)

The `em_msg_t` reader functions (batch B13) read `m_buff` and `m_len` members. The plan is to
extract each reader like a sender (`em_msg_t::` stripped) and to declare in `shim.hpp` only
`struct em_msg_t { unsigned char *m_buff; unsigned int m_len; };`, with the reader's range
checking that it uses nothing else. The harness then fills `m_buff` with a TLV area from the
corpus. The `ba4180b` source decides which other members a reader touches; if one needs more than
these two, extract the class members the same way instead of widening the hand-written shim.
