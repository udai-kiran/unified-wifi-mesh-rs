# 0014: A `Reset.json` extension for per-AP profiles, and an AL MAC derived from the base MAC

| | |
|---|---|
| Status | **Accepted** (2026-11-22, user), rev 8 after two rounds of pre-implementation review. Open choices 3 and 6 wait for the Stage 1 result; the 0012 D9 amendment is conditional on Phase C. Not implemented |
| Date | 2026-11-22 |
| Upstream | `rdkcentral/unified-wifi-mesh` @ `ba4180ba36748629f10c20babc4320dd69c9db4f` |
| 1905 layer | `rdkcentral/ieee1905-rs` @ `a6443225db82f7e8aed1c29644d2ed0b947b4d6e` |
| Platform source read | `rdkcentral/OneWifi` @ `59be72f` |
| Follows | [0013](0013-s2-onboarding.md) D2, D6, D7, E1; [0012](0012-s1-discovery.md) D9; [0011](0011-em-transport-s0.md); [0010](0010-vertical-slices-and-transport.md) D1 (slice order) |
| Amends, if accepted | 0013 D2 (an optional port section in the reset file), D4 (a Renew re-runs WSC on every radio), D6 (`m2-row-search`: rows come from the device's profile), D7 (per-device SSID check, Response MID, fresh M1, mismatch latch, owner update, per-agent `em_config` age, renew retry); 0013 E2 (renew retry decided); 0012 D9 (the registered AL MAC is checked against the expected one, Phase C); 0010 D1 (slice order); 0002 D2 (`em-cmd` carries the admin protocol) |
| Affects | `em-db` (section, source bytes and splice), `em-types` (a MAC helper), `em-cmd` (admin protocol), `em-orch` (per-node `em_config` age), `em-core` (M2 row order, renew counter), `em-agent` (Renew on every radio, multi-radio timeout), `em-ctrl`, new `em-cli` crate, `docker/`, and a patch to `ieee1905-rs` (Phase C) |
| Spec | EasyMesh §5, §6.1, §7.1, §13; IEEE 1905.1 §3.1, §4.4, §7.1.1, §10.1.3 |

## Context

### What the controller does today

`em-ctrl` loads one network-wide `NetworkSSIDList` from `/nvram/Reset.json` (0013 D2). For each M1, `build_m2`
(`crates/em-core/src/wsc.rs:701-748`) walks the haul indices `0..min(max_bss, 5)` and takes the first enabled row
matching that haul, the radio's band and the M1's authentication flags. This has four limits:

1. **No per-AP choice.** The sender is not an input, so two APs with the same bands and authentication get the same
   networks.
2. **One network per haul type per radio.** The haul types are fronthaul (0), backhaul (1), IoT (2), configurator (3)
   and hotspot (4) (`inc/em_base.h:2489-2494`). A second network on a radio must use another haul type.
3. **The haul index is capped by `max_bss`.** With `max_bss = 2`, only hauls 0 and 1 are considered, so a hotspot
   network is dropped even when the radio has a free BSS.
4. **The SSID check is global.** A Topology Response passes if each SSID appears anywhere in the list
   (`crates/em-core/src/topology.rs:292-302`, as upstream).

### What the standards require

- The controller answers each radio's M1 with one M2 per BSS to configure, or one Tear Down M2, within one second, and
  sends no more M2s than the radio's advertised maximum (EasyMesh §7.1). Each M2's N2 is unique.
- How the controller **chooses** those BSSs is not defined. §7.1's footnote says the specification "only partially
  defines" the controller's algorithms. A per-AP policy is therefore allowed, and so is a shared list.
- The controller "shall configure at least PSK on at least one fronthaul BSS" (§7.1).
- The AL MAC is "a locally administered" EUI-48 that uniquely identifies the abstraction layer (1905.1 §3.1). It must
  not conflict with any other MAC or AL MAC in the 1905 network (§4.4).
- A role configured out of band "shall be stored in non-volatile memory" and used after a restart (EasyMesh §6.1).

"Multi-AP Profile" in EasyMesh means the **feature level** (Profile-1 to 3). This note calls the new concept a
**device profile** and never shortens it to "profile" where the two could be confused.

### How the AL MAC is set today

The pinned daemon uses the MAC of its `--interface` as the AL MAC (`ieee1905-core/src/main.rs:126`,
`interface_manager.rs:65-78`) and has no option to change it. `em-ctrl` and `em-agent` learn it at registration.

RDK's Banana Pi script derives every Wi-Fi MAC by adding to the `erouter0` or `eth0` MAC, uses `wifi1.3`'s MAC as the AL
MAC, and writes it to `/nvram/EasymeshCfg.json` (OneWifi `config/openwrt/banana-pi/wifi_interface_up.sh:17-55,124-153`).
That AL MAC is not forced to be locally administered, and it equals an interface MAC.

### Evidence from this host and disposable experiments

Run on 2026-11-22. Nothing in the repository or the pinned daemon was changed.

| Observation | Result |
|---|---|
| Host physical NICs (`enp5s0`, `wlp6s0`) | `addr_assign_type` = 0 |
| Host bridges and veths | `addr_assign_type` = 3 (set) |
| Container `eth0` (veth, MAC from compose) | type 3 |
| Same container after `ip link set … address` | still type 3, and `ip -j link` reports no `permaddr` |
| Device tree on this x86 host | absent (`/sys/firmware/devicetree/base` does not exist) |
| Daemon with only the AL MAC changed | registers with the new AL MAC, but **onboarding fails**: `em-ctrl` never receives a Search |
| Cause | SDU delivery treats the Ethernet source as the sender's AL MAC and compares the frame destination with the AL MAC (`cmdu_handler.rs:971-982`) |
| Both daemons with that delivery path fixed (20 changed lines) | **Two-radio onboarding completes** to `topo_synchronized` on both sides, with AL MACs `06:42:…` over interfaces `02:42:…` |
| **Patched controller daemon, stock agent daemon** | **Onboarding fails.** In 75 s the agent sent 154 Searches and the controller sent 154 Responses, but the agent never saw one |

The sender's Ethernet source is its interface MAC (`cmdu_proxy.rs:901`), and a stock receiver looks that up as an AL MAC
(`cmdu_handler.rs:980`). **A separate AL MAC therefore needs the receive fix on every daemon in the network.**

The experiment's patch is at `/home/udai/MEGA/learning-tracking/wifi-easymesh-r6/init-config/spike-ieee1905-al-mac.patch`.
It reads the AL MAC from an environment variable and is evidence only. It proves one onboarding, not the daemon's other
paths: `run-s0/s1/s2`, the eviction run, LLDP and link metrics were not run with it. `cmdu_handler.rs` has 78 non-test
lines naming `source_mac` (45 outside logging), and each needs checking. Two daemon `ERROR` lines ("missing
Profile2ApCapability TLV") appeared on M2. That handler logs this for any WSC without the TLV (`cmdu_handler.rs:840-841`),
so the patch does not cause them.

## Decision

### D1. Scope and phases

The work is three phases, so per-AP configuration does not wait for the daemon:

- **Phase A:** the `Reset.json` extension, admission and per-AP selection (D2 to D6, D10 to D18). Works with the stock daemon.
- **Phase B:** read the base MAC, derive the AL MAC, and run D9 **step 1 only**: resolve once per boot, store it, log it. Nothing applies it.
  The stock daemon keeps the interface MAC as the AL MAC.
- **Phase C:** D9 steps 2 to 4 (daemon `--al-mac` and the registration check). **It needs the receive fix on the controller's and every
  agent's daemon**, so it cannot ship for the controller alone (open choice 3).

**Section presence is the branch.** With `em-rs:Config` present, D4 to D6, D14 to D16, and D17's wake and fresh-M1 rule apply. Without
it, the controller keeps today's policy: global haul-order selection, the global SSID check, no admission gate, no admin changes and no
file-mode check. D18 (S2 hardening), D17's agent change (Renew on every radio, multi-radio timeout) and the bounded read apply in **both**
modes, and after Stage 3 so does the per-device retry.

Agent base-MAC derivation stays out of scope (open choice 6), but the agent's daemon patch is in scope for Phase C.

### D2. One file: `Reset.json` with an optional port section (user decision)

There is **no separate `init_config.json`**. The port adds one top-level member, `em-rs:Config`, beside upstream's
`wfa-dataelements:Reset`. The path and flag stay as they are: `/nvram/Reset.json` and `--reset-file`.

**Nothing rejects the new key** (checked 2026-11-22):

- Upstream looks up `wfa-dataelements:Reset` by name and reads named children (`dm_easy_mesh.cpp:862-864,894-958`).
- The port's loader accepted a new key at the top level, inside the Reset object and inside a row, in a scratch probe.
- Upstream's CLI decodes and re-encodes the file's known fields in memory, so it drops the section from what it sends but does
  not rewrite the file (`src/cli/em_cli.cpp:52-61`).

**Key matching.** The port matches `em-rs:Config` exactly and case-sensitively. A second top-level member with the same name, or one that
differs only in ASCII case, is a load error, because upstream's lookup ignores case and a stale copy could carry another policy.

**Size limit if upstream reads the same file.** Upstream reads the file with an unbounded `strncat` into `subdoc->buff`, which sits 64 octets
into a 4096-octet stack buffer (`em_cmd_exec.cpp:54-76`, `em_base.h:421`, `em_cli.cpp:43-52`). So at most **4031 octets** of file fit, plus the
terminating NUL; upstream's sample file is 3064. A larger file can **overflow upstream's stack**. With `shared_with_upstream` true, the limit is enforced at load (D4) and on every
save (D16).

- Read once at startup. There is no hot reload; changes made through `em-cli` (D16) apply without a restart.

### D3. Schema (version 1)

```json
{
  "em-rs:Config": {
    "version": 1,
    "base_mac": { "strategy": "json_file", "mac": "d8:3a:dd:a2:05:5c" },
    "device_profiles": {
      "default": { "bands": { "2.4": ["Home", "Backhaul"], "5": ["Home", "Guest", "Backhaul"] } },
      "garage":  { "bands": { "2.4": ["Guest"] } }
    },
    "devices": {
      "02:42:c0:a8:65:02": { "name": "garage-ap", "state": "approved", "device_profile": "garage" },
      "02:42:c0:a8:65:09": { "name": "old-ap", "state": "rejected" },
      "02:42:c0:a8:65:0a": { "state": "auto_rejected" }
    },
    "admission": {
      "discovery_timeout_secs": 86400,
      "auto_rejected_capacity": 64,
      "shared_with_upstream": true,
      "admin_uid": 1000,
      "vendor_manifests": []
    }
  },
  "wfa-dataelements:Reset": {
    "Interfaces": {},
    "NetworkSSIDList": [
      { "SSID": "Home", "PassPhrase": "change-me-home", "Band": ["2.4", "5"], "Enable": true,
        "HaulType": ["Fronthaul"], "AuthType": "WPA2 Personal" },
      { "SSID": "Guest", "PassPhrase": "change-me-guest", "Band": ["2.4", "5"], "Enable": true,
        "HaulType": ["Hotspot"], "AuthType": "WPA2 Personal" },
      { "SSID": "Backhaul", "PassPhrase": "change-me-backhaul", "Band": ["2.4", "5"], "Enable": true,
        "HaulType": ["Backhaul"], "AuthType": "WPA3 Personal" }
    ]
  }
}
```

This example is complete and is a test fixture for both readers: the port's loader, and upstream's `decode_config_reset`, which requires
`Interfaces` (`dm_easy_mesh.cpp:914-917`).

- **Networks stay in `NetworkSSIDList`**, so there is no second copy of SSIDs or passphrases. Device profiles refer to rows by SSID.
- `device_profiles` map a band (`2.4`, `5`, `6`) to an **ordered** list of SSIDs. An absent band or an empty list means a Tear Down M2
  for that band's radios.
- `devices` holds the **durable decisions**, keyed by AL MAC. `state` is `approved` (with a `device_profile`), `rejected` or
  `auto_rejected`. An `approved` or `rejected` entry written by hand is a pre-configured device. At most 256 `approved` plus `rejected`
  entries, and at most `auto_rejected_capacity` (default 64, at most 256) `auto_rejected` entries.
- `base_mac` is **optional**. Without it nothing is derived (Phase B is off). It is validated and resolved only by `--resolve-al-mac`
  (D9 step 1), so a bad value never stops the controller from serving Phase A. The controller's own loader treats `base_mac` as opaque
  JSON; only `--resolve-al-mac` decodes it, including the strategy name.
- `admission`: `discovery_timeout_secs` is 60 to 2592000. `shared_with_upstream` (default true) makes the 4031-octet limit a hard limit
  (D16). `admin_uid` and `admin_group` are optional (D16). `vendor_manifests` must be empty until the vendor path is built.
- **Capacity:** the loader keeps upstream's limit of **five rows** (`reset-bounds`, `MAX_NET_SSIDS`), so the network has at most five
  distinct networks to share among its APs. Raising it is a separate decision.

### D4. Validation at load (all failures are fatal)

The `wfa-dataelements:Reset` object keeps its upstream-compatible parsing. These rules apply only when `em-rs:Config` is present:

- `version` must be 1. Unknown fields **inside `em-rs:Config`** are rejected, so a typo cannot silently disable a rule.
- **Duplicate JSON keys are rejected** in `device_profiles`, each `bands` object and `devices`. `serde_json`'s default map decoding
  keeps the last value, so this needs a custom visitor. AL MAC keys are compared after parsing, so `AA:…` and `aa:…` are duplicates.
- Every reference resolves to one row. A referenced SSID must be unique in the list, have **exactly one** `HaulType`, and include the
  band that names it. A band list names a row once, and two rows in one band list cannot share a haul type (decision 5).
- Referenced rows use exact values. `AuthType`: `Open`, `Enhanced Open`, `WPA2 Personal`, `WPA3 Personal`, `WPA3 Transition`. `HaulType`:
  `Fronthaul`, `Backhaul`, `IoT`, `Configurator`, `Hotspot`. The existing helpers cannot be reused here: `authentication_type` maps any
  unknown string to SAE and `network_band` matches prefixes (`crates/em-core/src/wsc.rs:518-540`).
- **The effective authentication is validated per band.** On 6 GHz the M2 builder converts `Open`, WPA2 and transition rows to SAE
  (`wsc.rs:719-735`), so a row listed under `6` must have a valid SAE passphrase even if its `AuthType` is `Open`. Otherwise a keyless SAE
  M2 could be sent.
- A passphrase is 8 to 63 printable ASCII octets (0x20 to 0x7E) or exactly 64 hex digits, required unless the effective authentication is
  `Open` or `Enhanced Open`. This fits em-core's 64-octet key limit. The rule is the usual WPA form; the 802.11 text is not in the
  `usp-spec` index and has not been checked against it.
- **PSK on a fronthaul (decided by the user):** §7.1 requires PSK on at least one fronthaul BSS. Load fails unless at least one device
  profile lists an **enabled** PSK fronthaul row on 2.4 or 5 GHz. The check uses the file's profiles only, not device state, so an admin
  change can never make the file unloadable. 6 GHz stays SAE-only. A single radio
  whose `max_bss` cut removes its PSK fronthaul still only logs a warning.
- AL MAC keys must be unicast and non-zero.
- The file is read with a bound of **64 KiB** in **both** modes, before parsing (closing S2-R2-03). With `shared_with_upstream` true, a file
  over 4031 octets is rejected at load too.
- The file must be a regular file with no group or other permissions (`mode & 0o077 == 0`). It holds passphrases, and `std::os::unix`
  exposes this without `unsafe`.
- **No error or log contains a passphrase.** `serde_json` errors can quote a rejected value, so a test asserts this.

### D5. Selecting the networks for an M1

1. **Key:** the CMDU's source AL MAC from the transport (`remote`), **not** M1's MAC Address attribute. An upstream agent
   puts its radio MAC there (`em_configuration.cpp:3347-3350`).
2. Apply the admission gate (D14). Anything other than an admitted device gets step 6.
3. Take the device profile's list for the M1's band.
4. Keep networks that are enabled and whose effective authentication the M1 allows.
5. Send the first `max_bss` in **list order**, and log any that are skipped or cut by name.
6. If nothing remains, send one Tear Down M2 (§7.1).

With the section present, `build_m2` takes rows in the order given, rather than by haul index up to `max_bss`. This removes limit 3 there.
**With the section absent**, selection keeps today's haul-order rule, and a new test pins it with rows out of haul order and a capped
`max_bss`, because the existing test rows are already in ascending haul order.

A radio with five M2s exceeds 1500 octets, so the CMDU must be fragmented at TLV boundaries (1905.1 §7.1.1). The fake agent advertises
at most four BSSs per radio (`crates/em-agent/src/wsc.rs:19-20`), so D11 adds a five-BSS test radio.

### D6. Per-device Topology check

With the section present:

- For each radio, the controller **records the SSIDs it actually sent in the last M2**, replaced on each new M2. This reflects the
  `max_bss` and authentication filtering and needs no band lookup.
- The Topology Response is checked **per radio** against that record (tag `topology-ssid-per-device`). An unknown RUID is a mismatch.
  As upstream, a configured BSS that is missing from the report is not a mismatch.
- **A Response completes only the current configuration.** The controller keeps the MIDs of every Topology Query it has sent to the device
  since that device's last M2 (at most 64), and a new M2 to any radio of the device clears the set. A Response whose MID is not in the set
  is ignored (tag `topology-response-mid`), so a late
  zero-BSS report from a Tear Down cycle cannot complete an approval cycle. Today any Response completes every waiting radio
  (`crates/em-ctrl/src/s2.rs:262-285`).

With the section absent, today's global check stays.

### D7. Reading the base MAC at startup

The base MAC is read through a **pluggable strategy**. `em-rs:Config.base_mac.strategy` names exactly one. **There is no automatic
guessing or fallback across strategies**, so the AL MAC cannot change because a board gained an interface. (RDK's script has that
hazard: it prefers `erouter0` when present.)

**Version 1 implements one strategy, `json_file`**, which reads the `mac` field from this same file (D3).

**Rules for every strategy:**

- A strategy only returns a base MAC. Validation (unicast, non-zero), derivation (D8) and storage (D9) are shared and live outside it.
- Strategies are variants of one closed enum, decoded from the `strategy` tag, so an unknown name is an error in `--resolve-al-mac`. Adding one means
  adding a variant, its fields and its tests.
- Reads go through a configurable filesystem root, so tests run without real hardware.

**Possible later strategies.** None is designed or built here; each gets its own decision when a platform needs it:

| Strategy | Note for that later decision |
|---|---|
| NAND or NVMEM | A plain read of `/dev/mtdX` returns data even after an uncorrectable ECC error (`drivers/mtd/mtdchar.c`), so it needs its own integrity check |
| Network interface | `addr_assign_type = 0` is also the kernel's default (`linux/netdevice.h:60`), so it is not proof the MAC is permanent |
| Device tree | Absent on x86 |

### D8. Deriving the AL MAC

```text
reject a multicast or zero base MAC
if bit 1 of octet 0 is clear (factory MAC): set it     d8:3a:… -> da:3a:…
if it is already set (local MAC):                      fatal (user decision 7)
```

- **Uniqueness holds only among factory MACs.** A local MAC can equal another device's derived AL MAC: factory `d8:…` derives
  `da:…`, which is also a valid local address. This is why the already-local case is not silently accepted.
- The result is compared with every local interface MAC. A match is fatal, because some Wi-Fi drivers make extra BSSIDs with
  the same bit. The check runs **once**, when the value is resolved (D9 step 1). A BSS created later is not covered.
- Fatal here means `--resolve-al-mac` fails. The controller itself still starts and serves Phase A on the interface-MAC AL MAC.
- If a peer reports the controller's AL MAC in a Search, or a port MAC equal to it, the controller logs an error and ignores
  that message (1905.1 §4.4). This is part of Phase C.
- The function is pure and lives beside `MacAddr` in `em-types`, with tests for each case.

### D9. Startup order and the daemon

The daemon starts **before** `em-ctrl` and needs the AL MAC when it starts. To keep one identity per boot, the value is
resolved **once** and stored:

```text
1. em-ctrl --resolve-al-mac --reset-file /nvram/Reset.json --output /run/em/al_mac        (Phase B)
      if the file exists: validate it and keep it. Otherwise: D7, D8, then publish by hard-linking a temp file to the name, which fails if it
      already exists; a resolver that loses the race rereads the file and keeps the winner's value
2. ieee1905 --interface eth0 --al-mac "$(cat /run/em/al_mac)"                             (Phase C)
3. em-ctrl --reset-file /nvram/Reset.json --al-mac-file /run/em/al_mac                     (Phase C)
4. registration returns an AL MAC, which must equal the file's value, otherwise exit 1     (Phase C)
```

- `/run` is cleared at boot, so the value lives for one boot. A daemon or `em-ctrl` restart rereads it and never resolves again.
- `/run/em` is owned by root with mode 0755; `/run/em/al_mac` is root-owned, mode 0644. The admin socket lives elsewhere (D16).
- **Changing `base_mac`** takes effect at the next boot. To apply it sooner: stop `em-ctrl` and the daemon, delete the file, and
  run steps 1 to 3. Editing `Reset.json` alone changes nothing until then, so it cannot cause a crash loop.
- In Phase B only step 1 runs, and the value is logged. Step 4 is never enabled with a stock daemon, which returns the interface MAC.
- Step 4 extends 0012 D9, which already treats a changed AL MAC as fatal.

The daemon patch (open choice 3), **applied to every node in the network**, must:

1. Add `--al-mac` and use it at `main.rs:126` and the registration and role paths in `al_sap.rs:261,269,277,348`.
2. Accept frames addressed to the forwarding MAC as well as the AL MAC.
3. Resolve an incoming frame's source to the sender's AL MAC before delivering the SDU.
4. Audit every use of the frame source as an AL MAC in `cmdu_handler.rs`, and check LLDP, whose chassis ID is the AL MAC
   (`main.rs:147`).

It is applied in the Docker build after the existing pinned-hash check, kept in `docker/patches/`, and offered upstream. The
pin stays unchanged.

### D10. Recorded differences from upstream

Each gets an `intentional-difference` tag, an upstream cite and a test: `reset-file-port-section`, `per-device-m2-selection`,
`admission-gate`, `admin-interface`, `renew-all-radios`, `renew-unicast-wake`, `renew-retry-per-device`, `psk-fronthaul-required`,
`topology-ssid-per-device`, `topology-response-mid`, `fresh-m1-supersedes`, `mismatch-cleared-by-m1` (S2-08), `em-config-age-per-agent`
(S2-04), `m1-owner-update` (S2-06), `radio-node-after-m1-parse` (S2-03), `al-mac-from-base-mac`, `al-mac-registration-check`.

### D11. Tests and Docker checks

Unit and core tests (virtual time, no sockets):

- **Loader:** every D4 rule, including duplicate keys in all three maps, top-level duplicates and case variants, MAC case, strict values,
  6 GHz effective authentication, the PSK-fronthaul rule, file size and mode, multibyte passphrases, the D3 example as a fixture, and no
  secret in any error. A file **without** the section behaves exactly as today, including the global Topology check.
- **Admission:** every row of the D15 table, every invalid transition, the timeout boundary, AutoRejected persistence and its capacity,
  peer-model protection, and that no M2 to a non-admitted device carries a credential.
- **Fresh M1 and wake:** a fresh M1 while a radio is in `wsc_m2_sent` with an `em_config` waiting; a lost wake retried and exhausted; a
  wake while the agent's `cfg_renew` is busy; approve and revoke while a credential-bearing or Tear Down M2 is in flight; a delayed
  Topology Response from an older cycle.
- **Admin interface:** unauthorized peer, oversized and malformed requests, save failures before and after rename, an external edit
  between load and save, escaped keys and BOM in sibling members, the hard size limit, and a delayed save while an M1 is answered on time.
- **Selection:** two devices with different networks, list order and `max_bss`, authentication filtering, Tear Down, and the legacy
  haul-order test.
- **Agent Renew:** two radios renewed, one M2 delivered and one dropped until the command expires; a Renew during recovery; no stale
  command after synchronization.
- **Resolver (Phase B):** the `json_file` strategy (valid, missing, malformed, oversized), an unknown strategy, each D8 case, the collision
  check, one value across same-boot restarts, and the `base_mac` change procedure.

Docker checks, by phase:

- **Phase A:** discover, list, approve and onboard a two-radio AP through `em-cli` with no process restart; reject; timeout; a lost
  approval wake recovered; two agents on different device profiles, each receiving only its own SSIDs (checked by SSID, closing
  S2-12); a five-BSS radio across fragmentation; a mismatch that recovers on an M1 from a non-first radio; `run-s0`, `run-s1` and
  `run-s2` still passing.
- **Phase B:** the resolved value is logged and unchanged across same-boot restarts.
- **Phase C:** all nodes on the patched daemon with derived AL MACs, and a mixed-daemon run that must fail clearly.

### D12. Migration from a plain `Reset.json`

No conversion is needed. An existing file keeps working unchanged. To enable per-AP policy, add `em-rs:Config`, make each row it
references single-haul, and set the file mode to 0600.

### D13. Harness migration

- `run-s0`, `run-s1` and `run-s2` keep the current path and file, so they need no change.
- A new Phase A harness mode uses a `Reset.json` with the section, installed with mode 0600, because a plain `COPY` keeps the repository
  file's mode. Daemon `--al-mac` is used only by a later Phase C mode.

### D14. Admission gate (user decision)

**A radio is onboarded only if its device is admitted.** The decision is per device (AL MAC) and is enforced on every M1:

```text
Rejected?                     -> Tear Down
Approved by an admin?         -> M2 from its device profile
Pre-configured in Reset.json? -> M2 from its device profile
Vendor proven by signature?   -> M2 from the vendor's device profile (after DPP; off by default)
Otherwise                     -> Discovered (or AutoRejected), Tear Down, wait for the admin
```

- Rejected is checked first, so a revoked device cannot readmit itself through another path.
- A Search still gets a Response in every state (§6.1), and an M1 is always answered within one second (§7.1). Only an admitted device
  receives credentials.
- **Same vendor means "same vendor and sold to this owner":** a vendor-signed manifest of device keys, built after DPP and off by default.
  An OUI cannot prove a vendor, because it can be forged and setting the local bit changes it. M1's Manufacturer is free text. A genuine
  device is also not necessarily this owner's device.
- **Peer models of admitted devices are not evicted.** Today the 256-entry peer-model table evicts the oldest model whatever its state
  (`crates/em-ctrl/src/lib.rs:302-330`), and an M1 for an unseen radio without a model is dropped (`s2.rs:86-89`). With the section
  present, eviction skips Approved and Rejected devices; if only those remain, a new Search is answered but no model is created.
- **Every valid M1 is still answered.** With the section present, an M1 from a device with no peer model gets a Tear Down built from the M1
  alone, with no model and no radio node created, so §7.1's one-second answer holds for any device.
- If `em-rs:Config` is absent there is no gate.

### D15. Approval state machine (user decision)

This is separate from the per-radio protocol state (`CtrlState`). A refused device can be protocol-synchronized with zero BSSs.

```text
(unknown)                --first Search-------------->  DISCOVERED
(unknown)                --approve / reject by MAC--->  APPROVED / REJECTED
DISCOVERED               --approve / reject---------->  APPROVED / REJECTED
DISCOVERED               --timeout (24 h)------------>  AUTO_REJECTED
AUTO_REJECTED            --approve / reject---------->  APPROVED / REJECTED
REJECTED                 --approve------------------->  APPROVED
APPROVED                 --approve, new profile------>  APPROVED
APPROVED                 --revoke-------------------->  REJECTED
APPROVED                 --approve, same profile----->  APPROVED (no change)
REJECTED                 --reject-------------------->  REJECTED (no change)
REJECTED, AUTO_REJECTED  --forget-------------------->  (unknown)
```

The table below is authoritative; the diagram lists the same edges.

| From | Event | To | Side effect |
|---|---|---|---|
| none | First Search | Discovered | Log; shown by `em-cli` |
| none | Approve + device profile | Approved | Save; wake if the device has radio nodes, otherwise it is admitted on its next M1 |
| none | Reject | Rejected | Save |
| Discovered | Approve + device profile | Approved | Save, then wake (D17) |
| Discovered | Reject | Rejected | Save |
| Discovered | Timeout | AutoRejected | Save (bounded, see below), log |
| AutoRejected | Approve + device profile | Approved | Save, then wake |
| AutoRejected | Reject | Rejected | Save |
| Rejected | Approve + device profile | Approved | Save, then wake |
| Approved | Approve + another device profile | Approved | Save, then wake |
| Approved | Revoke | Rejected | Save, wake so it receives Tear Down, and warn that passphrases must rotate |
| Rejected, AutoRejected | Forget | none | Save; next contact is Discovered |
| Approved | Approve, same profile | Approved | No change; reported as already approved |
| Rejected | Reject | Rejected | No change; reported as already rejected |
| any other pair | | | Invalid; the request fails and nothing changes |

- The timer runs from **first contact** and is never extended. The default is **24 hours**. It uses monotonic time through the core's
  existing `tick` and `next_deadline`.
- **AutoRejected persists (decided by the user):** it is saved to `devices` and survives restart and table pressure. At most
  `auto_rejected_capacity` (default 64) entries are kept; when full, the oldest AutoRejected entry is dropped. Each device is written at
  most once, at its timeout, so a Search flood can only churn this capped list.
- **Byte budget:** when a save would exceed the size limit, the oldest AutoRejected entries are dropped until it fits, before any admin
  decision is refused. "Oldest" is the entry's order in the file, which the splice writer keeps. Under `shared_with_upstream` the real
  capacity is small: with upstream's 3064-octet sample rows, about a dozen AutoRejected entries fit.
- **Discovered is memory only**, at most 256 entries, oldest evicted first. A controller restart forgets Discovered devices; one that makes
  contact again starts a new timer; this is an accepted limit of the one-file design. A torn-down device that never searches again can still be approved by AL MAC (`none` → Approved).
- Approved and Rejected entries are never evicted. If they reach their cap of 256, approve and reject requests for new devices fail with
  a clear error.
- A Tear Down caused by this gate logs "pending approval" or "rejected", not the misleading "no fronthaul M2 carries PSK" warning.
- The transition is a pure function, so every row is tested without sockets.

### D16. Admin interface and `em-cli`

```text
em-cli devices list [--state discovered|approved|rejected|auto-rejected]
em-cli devices show <al-mac>
em-cli devices approve <al-mac> --profile <name> [--name <label>]
em-cli devices reject <al-mac>
em-cli devices revoke <al-mac>
em-cli devices forget <al-mac>
em-cli profiles list
```

- **Transport:** a local UNIX socket at `/run/em-admin/admin.sock`. There is no network listener. A remote interface (for example USP)
  is a later decision.
- **Authorization:** the init script (in Docker, the entrypoint) creates `/run/em-admin` owned by root and the group named by
  `admission.admin_group` (default `root`), mode 2750, so the setgid bit gives the socket that group. `em-ctrl` removes a stale socket and
  binds a new one with mode 0660. The controller also checks
  the peer's UID with tokio's `UnixStream::peer_cred` and accepts root and the optional `admission.admin_uid`. tokio's `net` feature is
  already in the workspace through `em-transport`, so no new crate is needed. A non-root end-to-end test covers the configured UID.
- **Section absent:** the admin interface is not started, and `em-cli` reports that no policy section exists.
- **Protocol:** one versioned, line-based text request per connection, at most 512 octets, at most 4 connections. It uses only `std`.
- **Actor scheduling:** connections are read and parsed in their own small tasks with a 2 s limit, outside the role actor's loop. Each sends
  a typed request into the actor over a bounded channel with a reply channel. The actor applies it in `CtrlCore` and starts the save. The
  save runs in a blocking task, and its completion comes back to the actor as another input. The actor never awaits socket reads or file
  I/O, so it keeps answering M1s on time. One save is in flight at a time; other requests wait or get "busy".
- **Saving:**
  1. Re-read the file. If its bytes differ from the last loaded or saved version, refuse with "changed on disk", so an external edit is
     never overwritten.
  2. Splice the new `em-rs:Config` value into the **original bytes**, replacing only that member's value span. Sibling members keep their
     exact bytes, spelling, escapes, whitespace and BOM. The loader therefore records the source bytes and the span of that member.
  3. Check the size: above 3 KiB, warn; above 4031 octets with `shared_with_upstream` true, or above 64 KiB, refuse.
  4. Write a temp file in the same directory with mode 0600, flush it, rename it over the file, then flush the directory.
- **Before the rename**, the spliced bytes go through the full loader, so a save can never write a file that fails to load.
- **When a change takes effect:** an approve or re-approve takes effect only after the rename, so no credentials go out under a policy that
  might not be saved. A revoke or reject takes effect at once, because it only withholds credentials, and is then saved. A failure before
  the rename returns an error: an approve stays unapplied; a revoke or reject stays in force in memory and its save is retried. A failure
  after the rename (the directory flush) keeps the change, and the reply says it may not survive a power loss.
- **Internal saves** (Discovered to AutoRejected at timeout) queue behind any save in flight. If one fails, the device stays AutoRejected
  in memory and is written by the next successful save.
- **External edits:** the byte check catches most edits made outside `em-cli`, but it is best effort, because an edit landing between the
  check and the rename can be lost. Edit the file only while `em-ctrl` is stopped. After "changed on disk", saves are refused until `em-ctrl`
  restarts and reloads the file.
- Output never contains a passphrase. Device-supplied text is shown escaped, because a Search or M1 is untrusted.
- `em-cli` is a new workspace crate. Its original scope, a port of upstream's `src/cli/em_cli.cpp`, becomes a later decision; this amends the
  STATUS row and the crate README. The protocol types live in `em-cmd`, which amends 0002 D2's scope for that crate.

### D17. Waking an approved AP: Renew covers every radio (user decisions)

After Tear Down an AP settles and does not ask again, so approve, re-approve and revoke **wake** it with an AP-Autoconfiguration Renew.

- **Unicast to that AP (decided by the user).** Only the affected AP re-runs onboarding. IEEE 1905.1 §10.1.3 says the Renew is relayed
  multicast; sending it unicast matches upstream and the port's current path (`crates/em-core/src/configuration.rs:175-182`). This does **not** conform
  to 1905.1 §6.3.10 and §10.1.3; it is a deliberate compatibility choice, and Stage 4 tests that prplMesh agents accept it. Tag
  `renew-unicast-wake`.
- **Bounded retry, per device (decided by the user).** The wake uses the D18 retry: up to five Renews for that device, counted from the
  first send. It stops when **every** radio the controller knows for that device has sent a fresh M1 since the wake (user decision, revised
  2026-11-22). After five sends it stops and logs the radios that never answered. A new wake for the same device replaces a running one
  instead of adding a second. Tag `renew-retry-per-device`.
- **Agent change:** on a Renew, re-run WSC for **every radio**, as §7.1 requires ("for each of its radios, irrespective of the value specified
  in the SupportedFreqBand TLV"). Upstream and the port pick one radio by band (`em_agent.cpp:2400-2447`). Tag `renew-all-radios`.
- `cfg_renew` carries a list of radios. A radio in a state that refuses a Renew is skipped and logged. If the agent's `cfg_renew` is already
  running, the Renew is dropped, and the controller's retry covers it.
- **Multi-radio timeout:** when the shared `cfg_renew` expires, every unfinished radio has its WSC session cleared and returns to
  `unconfigured`, not only the radio that triggered the timeout, and `reopen_search` takes ownership of recovery exactly once
  (`crates/em-agent/src/rules.rs:208-241`, `src/lib.rs:378-429`).
- **Confirmation matches the current applies.** The agent answers a Topology Query, or submits `onewifi_cb`, only after the platform has
  confirmed every radio's latest apply. Each apply carries an identifier that its confirmation echoes; today the fake platform's snapshot
  has none (`crates/em-agent/src/platform.rs:169`, `src/lib.rs:262`).
- **A fresh M1 supersedes the old exchange** (tag `fresh-m1-supersedes`). Today an M1 for a radio in `wsc_m2_sent` is dropped before
  selection (`crates/em-ctrl/src/s2.rs:52-60,76-82`). With the section present, a validated M1 whose enrollee nonce differs from the last
  answered one restarts that radio at `wsc_m1_pending`. It removes the old `em_config` chain, updates the owner, clears the device's
  mismatch and goes through the gate and selection. An identical retransmission is still ignored.
- **Exit test:** after one `approve`, both radios of a torn-down AP reach `topo_synchronized` with their networks and no process restart,
  including when the first Renew is lost.

### D18. S2 hardening and the controller retry

These change S2 behaviour and are Stage 2 of the plan. Each is an amendment to 0013 and gets its tag, an upstream cite and a test.

- **Renew retry (decided by the user, 0013 E2):** after the 40 s `em_config` cancel, the controller retries the Renew up to five times, as
  upstream does (`em_orch_ctrl.cpp:171-175,404-422`; `em_configuration.cpp:938,6865`). It needs controller `CfgRenew` rules and a counter.
  In Stage 2 the count is per radio, as upstream, because the agent still renews one radio: the retry starts at the 40 s cancel, counts the
  first send toward five, sends one Renew per idle second while the radio is `misconfigured`, and stops when that radio reaches
  `wsc_m2_sent` or after five sends. In Stage 3 the cancel path and admin wakes share one per-device scheduler (D17), in both modes.
- **S2-03:** a radio node is created only from an M1 that parses, so it always has a band. The AP Radio Identifier used for routing must
  equal the RUID in AP Radio Basic Capabilities, or the M1 is rejected before any state changes.
- **S2-06:** an M1 updates a radio's owner atomically, so M2, the selection, Queries and Renews agree on one AL MAC.
- **S2-08:** a new M1 from any radio clears `ssid_mismatch` for **every radio of that device**. The flag is set on the first radio but
  blocks the whole device's Query (`crates/em-ctrl/src/s2.rs:219-224,290-295`).
- **S2-04:** the 40 s `em_config` age is per agent. `em-orch` keeps elapsed time per command type (`crates/em-orch/src/lib.rs:58-101,168`),
  so one agent held in mismatch can cancel another's. The fix adds a per-node start time for `em_config` in `em-orch`, chosen over a role
  timer because the cancel already lives there. It also covers the two remaining type-wide paths: the transient hook's whole-type cancel
  (`crates/em-ctrl/src/rules.rs:207-210`) and `remove_chain_for_node` resetting the type-wide elapsed time (`crates/em-orch/src/lib.rs:646-648`).
  Each becomes per agent, with a test.
- **S2-R2-ADV-01:** a Renew that arrives while a recovery `dev_init` runs is refused, so no stale `cfg_renew` runs after synchronization.

## Open choices

| # | Choice | Options | Recommendation |
|---|---|---|---|
| 1 | Configuration file | **Decided by the user (2026-11-22):** extend `Reset.json` with an optional section; no `init_config.json` | Decided |
| 2 | Unlisted AP | **Decided by the user (2026-11-22):** it stays Discovered and gets Tear Down until admitted (D14, D15) | Decided |
| 3 | Daemon AL MAC | Patch every daemon (Phase C); or stay on interface-MAC AL MACs for now | **Do A and B, and hold C** until the receive fix is accepted upstream or every node runs our patched daemon |
| 4 | Base MAC strategy | **Decided by the user (2026-11-22):** a pluggable strategy named in the config. `json_file` now; NAND and others when needed | Decided |
| 5 | Same haul type twice on a radio | **Decided by the user (2026-11-22):** forbidden in v1 to keep it basic; support what EasyMesh allows later. OneWifi applies each M2 to the VAP of that haul type, so a repeat would silently overwrite the earlier one (`wifi_easymesh_translator.c:3203-3205` at `59be72f`) | Decided |
| 6 | Agents | Controller only; or derive agent AL MACs too | **Controller only** for derivation. Agents still need the patched daemon in Phase C |
| 7 | Already-local base MAC | **Decided by the user (2026-11-22):** fatal. Provision a factory-style base in `em-rs:Config` instead | Decided |
| 8 | Slice order | **Decided by the user (2026-11-22):** DPP, 1905 keys and encryption come before S3 to S6, so those slices are developed and tested with protection on. 0010 D1 gets an amendment when this note is accepted | Decided |
| 9 | Vendor path | **Decided by the user (2026-11-22):** after DPP, as a signed "sold to this owner" manifest, off by default | Decided |
| 10 | Discovery timeout | **Decided by the user (2026-11-22):** 24 hours by default, configurable | Decided |
| 11 | AutoRejected | **Decided by the user (2026-11-22):** stays until an admin acts | Decided |
| 12 | Renew | **Decided by the user (2026-11-22):** the agent re-runs WSC on every radio (§7.1) | Decided |
| 13 | AutoRejected storage | **Decided by the user (2026-11-22):** persisted in `devices`, capped (default 64), oldest dropped (D15) | Decided |
| 14 | Wake addressing | **Decided by the user (2026-11-22):** unicast Renew to that AP, as upstream; a recorded difference from 1905.1 §10.1.3 (D17) | Decided |
| 15 | PSK fronthaul rule | **Decided by the user (2026-11-22):** a config with no PSK fronthaul on 2.4 or 5 GHz is rejected at load (D4) | Decided |
| 16 | Renew retry unit | **Decided by the user (2026-11-22, stop condition revised):** per device once Renew covers every radio, at most five Renews, stopping when every radio of the device has sent a fresh M1 (D17, D18) | Decided |

## Security

**Threat.** 1905 frames are not protected on plain Ethernet. In a Docker capture on 2026-11-22, a bystander on the same bridge saw
every multicast Search and Topology Discovery, including AL MACs. An on-path capture also saw the unicast traffic, including SSIDs
in the Topology Response. No passphrase appeared, because M2 encrypts it. That encryption stops passive listeners only: WSC here is
unauthenticated, so from reading the code, a device that sends a Search and an M1 is given the credentials, and a device can also
pose as the controller. These active attacks were not run.

**A device profile is not access control.** The AL MAC is unauthenticated and visible in Searches, so it can be copied.

**What the standards offer** (searched 2026-11-22):

- EasyMesh §5.3 and §13 already define the strongest fix: DPP with out-of-band bootstrapping, a MIC on multicast and AES-SIV on
  unicast. It remains conditional. A PBC-onboarded agent cannot use 1905-layer security unless it also does DPP (§5).
- The latest EasyMesh found is [v6.1](https://www.wi-fi.org/system/files/Wi-Fi%20EasyMesh%20Specification%20v6.1.pdf) (2025-12-15).
  It keeps PBC/WSC, keeps Search and Response unencrypted, and adds a rule that an Agent discards messages from devices not in the
  latest Agent List. Without a MIC, that rule relies on a spoofable address. This reading comes from a web research pass and was not
  checked through `usp-spec`.
- IEEE 1905.1-2013 and 1905.1a-2014 are [inactive, with no active project](https://standards.ieee.org/ieee/1905.1/4995/).
- The pinned [ieee1905-rs](https://github.com/rdkcentral/ieee1905-rs/blob/a6443225db82f7e8aed1c29644d2ed0b947b4d6e/README.md) has no MIC
  or encryption. Published research reports [password leakage through EasyMesh](https://i.blackhat.com/BH-US-24/Presentations/US24-Zhou-Fallen-Tower-of-Babel-Rooting-Wednesday.pdf).

Until slice S7, a wired deployment needs an isolated management VLAN or MACsec (802.1AE). EasyMesh defines neither for this purpose.

## Accepted risks (user, 2026-11-22)

- **Cross-testing partner:** prplMesh is the second implementation. At `3ff8bea` (2026-11-20) it claims R1 to R5 with R6 in development, has
  R6/MLO code and a dummy radio backend, and no Wi-Fi Alliance certificate naming it was found.
- **No independent peer for 1905 security:** neither prplMesh nor upstream at the pin uses the MIC or Encrypted Payload TLVs in role code, and
  prplMesh's controller does not use the DPP chirp. Stages 7 to 9 rely on spec-derived vectors and our own test peer. The port makes **no
  multi-vendor security claim** from them.
- **R6 reference:** upstream describes itself as R5-based and prplMesh's R6 is unfinished, so the specification is the reference for R6 behaviour.
- **Build:** prplMesh's documented Docker builder is Ubuntu 18.04, so building it is the first task of Stage 4.
- **Until DPP**, admission is by AL MAC, which can be copied. The gate stops accidents, not attackers.

## Alternatives considered

| Alternative | Why not |
|---|---|
| A separate `init_config.json` | Nothing rejects a new key in `Reset.json`, so a second file adds no safety and would duplicate passphrases |
| New keys inside `wfa-dataelements:Reset` | A top-level section keeps upstream's object untouched and keeps the port's schema separate |
| Key device profiles by radio ID | Finer than needed, and a device's radios are managed together; can be added later |
| Key device profiles by M1's MAC Address attribute | An upstream agent sends its radio MAC there |
| Key device profiles by base MAC | The controller never sees an agent's base MAC |
| Store device profiles in SQLite | No database exists yet; a later `em-db` step can import this file |
| Set the interface MAC to the derived value | Changes the device's visible MAC, leaves the AL MAC equal to an interface MAC, fails the D8 collision check, and would make a later network-interface strategy read type 3 on the next boot |
| Derive the AL MAC inside the daemon | Puts the port's policy into an external project |
| Let `em-ctrl` launch the daemon | Changes process ownership for one value |

## Follow-ups

- **Restart gap:** after a controller restart, agents already in `topo_synchronized` keep their old networks. Approved devices can be
  woken by D17, but a startup policy for this needs its own note (Stage 5).
- **Stale radios:** an agent that returns with fewer radios under the same AL MAC keeps its old radio nodes, which block the device-wide
  Query (S2-R2-01). Ageing model, radio links, deadlines, chains and SSID records together needs its own decision.
- **§6.1 role persistence:** the role is still chosen by which binary runs.
- `ip -j link` `permaddr` as a fourth base MAC source, once tested on real hardware.
- **Several BSSs of one haul type on a radio:** EasyMesh §7.1 allows it, and v1 does not. Add it later, for agents shown to handle it, because
  OneWifi keeps one slot per haul type.
- Check the EasyMesh v6.1 Agent List filter against the project's spec source, and decide whether the port adopts it.

## Acceptance criteria

1. The user has decided the remaining open choices.
2. D2 to D6 and D10 to D18 are implemented with no new dependency, on stable and 1.85.0.
3. D7, D8 and D9 step 1 are implemented, and `em-ctrl --resolve-al-mac` gives one value per boot.
4. If Phase C is chosen: `em-ctrl` refuses an AL MAC that differs from the resolved one, and the patched daemons pass D11.
5. `STATUS.md`, READMEs and amendment pointers in 0012 and 0013 (and 0010 if S7 moves) are updated.

## Errata

- **E1 (2026-11-25, Stage 2 implementation; plan review of the Stage 2 contract and user decisions Q1 to Q5, R1 to
  R4).** D18 as implemented:
  1. **S2-03 wording.** An M1 carries no AP Radio Identifier TLV. The rule is: the RUID the controller routes by (the
     first radio-bearing TLV, `readers::radio_id`) must equal the RUID of the M1's AP Radio Basic Capabilities TLV.
     The M1 is parsed and checked (RUID, peer model, also for a known radio, and at most 4 radios per agent,
     `EM_MAX_RADIO_PER_AGENT`), and its M2 is built and validated, before any state changes; a new radio node is
     created from the parsed M1. A failed check, build or validation changes nothing (user decision, 2026-11-25).
  2. **Renew retry mechanism (R1, R2).** No controller `cfg_renew` command: a per-radio counter on the controller,
     sent from the proto tick while the radio is `misconfigured`. Two rules start an episode: the 40 s
     `topo_sync_pending` timeout marks and renews every radio of the agent (upstream `em_orch_ctrl.cpp:75-92`); any
     other `em_config` cancel marks and renews exactly the candidates of active stages, through the cancel hook, and
     drops pending stages with no state change (upstream `em_orch.cpp:270-302`). The counter restarts at each episode (upstream
     carries it over); the episode stops when the radio leaves `misconfigured` or after 5. Upstream's 10 s
     `cfg_renew` cancel is not ported.
  3. **S2-04 mechanism.** `em-orch` gives the transient hook its own stage's age and lets the role restart the stage
     start of chosen candidates. An accepted M1 restarts the age of all the agent's `em_config` stages; the
     non-`topo_sync_pending` branch cancels only that agent's radios (item 2 gives which radios are marked). The type-wide elapsed value is unchanged.
  4. **Added to D18.** Peer-model eviction (Q1, R3): least recently heard from. A Search, an M1 whose M2 was
     built (not one that is refused or ignored) and a Topology Response refresh a model. Eviction removes the agent's chains, radio nodes, links,
     deadlines and retry state. S2-06 lets any agent with a peer model take a radio over (R4, an accepted risk
     until admission).
  5. **Cites.** D18's file:line cites were taken at `78a5c83`, before note 0016 moved the roles onto the event loop,
     and no longer match the code. The current places are named in the crate READMEs and the `intentional-difference`
     tags.

## Revision history

- Erratum E1 (2026-11-25): D18 as implemented in Stage 2.
- Accepted by the user (2026-11-22). Amendment pointers added to 0013 D2, D4, D6, D7; 0010 D1; 0002 D2.
- rev 8 (2026-11-22, round 2 of the pre-implementation review: plan reviewer 10 findings, Codex 17, none critical; this is the last review
  round for this design): section gating split (D18 and the agent change apply in both modes); PSK rule over enabled rows in the file's
  profiles; byte budget for AutoRejected; when approve, revoke and internal saves take effect, and a full-loader check before rename;
  external edits are best effort; Query MID set instead of one MID; M1 without a peer model answered with Tear Down; RUID fields must agree;
  confirmation per apply; retry stops when every radio answers (user decision); Stage 2 retry rules; remaining type-wide `em_config` paths;
  S2-R2-ADV-01 placed in D18; socket group and setgid; resolver publication; unicast Renew stated as nonconforming; fixture valid for
  upstream; diagram self-loops; 4031-octet wording. Not reviewed again.
- rev 7 (2026-11-22, pre-implementation review: plan reviewer 14 findings, Codex 22, none critical; user decisions 13 to 16): section
  presence is the legacy branch; Phase B includes D9 step 1 only and `base_mac` is optional; AutoRejected persisted and capped; complete
  transition table (re-approve, approve by MAC, no-ops); peer models of admitted devices protected; per-device wake with bounded retry and
  unicast Renew; a fresh M1 supersedes the old exchange; the Topology Response must match the outstanding Query's MID; multi-radio Renew
  timeout; admin socket ownership, actor scheduling, conflict check, byte-preserving splice, commit point and a hard size limit; 6 GHz
  effective authentication; the PSK-fronthaul rule; a five-BSS test radio; D18 for Stage 2 with the `em-orch` mechanism; a complete D3
  example. Not reviewed again.
- rev 6 (2026-11-22, user decisions): haul-type limit, already-local base MAC and slice order decided; prplMesh chosen; accepted risks recorded.
- rev 5 (2026-11-22, user decisions): admission gate (admin, pre-configured, or vendor-signed), the approval state machine with a 24-hour
  timeout, the `em-cli` admin interface, and a Renew that covers every radio. `unknown_devices` is removed. Not reviewed again.
- rev 4 (2026-11-22, user decision on open choice 1): no `init_config.json`. An optional `em-rs:Config` section in `Reset.json` holds the
  base MAC and device profiles, and profiles refer to existing rows by SSID. Checked that no reader rejects the key, and recorded upstream's
  4096-byte load buffer and the five-row limit. Not reviewed again.
- rev 3 (2026-11-22, user decision on open choice 4): the base MAC is read through a pluggable strategy named in the config. Version
  1 implements only `json_file`; NAND and other strategies are left for later decisions. Not reviewed again.
- rev 2 (2026-11-22, plan review: 1 critical, 2 high, 8 medium, 5 low; all addressed): a separate AL MAC needs the receive fix on
  every daemon (reproduced with mixed daemons), so Phase C is no longer controller-only; one resolved AL MAC per boot; harness
  migration; uniqueness claim corrected and the already-local case made a user choice; strict values and octet-based passphrases;
  Topology check against the SSIDs actually sent; Reset.json migration table; tags for the S2 prerequisites. Security section added
  from a wire capture and a search of later standards; the recommended unknown-device policy changed to Tear Down.
- rev 1 (2026-11-22): first draft, with the experiment showing the daemon needs more than an `--al-mac` option.
