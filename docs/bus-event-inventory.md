# `em_bus_event_type_t` inventory

Every enumerator of upstream `em_bus_event_type_t` (`inc/em_base.h:3140-3213` @ `ba4180b`; 71 enumerators = 70 values + `em_bus_event_type_max`), with where it is produced and consumed, and the replacement it maps to under [0001](0001-em-types-scope.md) D7. **The category is a fact about upstream. The replacement owner is provisional** until design note 0002 settles the `em-cmd` / `em-core` / `em-orch` boundary.

**How this was produced.** A text scan of `src/` (not compiled). An occurrence counts as a *consumer* if it appears in `case X` or `== X`. It counts as a *producer* if it appears in `io_process(`, a `type =` assignment, or a send/push call. Anything else (e.g. string tables) is listed under *other*. Commented-out code was manually excluded after review (revision 3 review, finding 3). A dash means no occurrence was found by the scan; it does not prove the value is unused. Each row is confirmed or corrected when `em-cmd`/`em-core` is ported.

**Categories.**

- **A.** Management request (CLI / rbus / TR-181 → controller or agent)
- **B.** Platform notification (OneWifi callback → agent)
- **C.** Engine → host action (`src/em/*` asks the manager to act)
- **D.** Manager-internal
- **E.** Unused or sentinel

Counts: A 27, A+B 1, A+C 4, B 15, C 12, D 5, E 7 (total 71).

A value with two roles (e.g. `A+C`) becomes two separate Rust types.

| Enumerator (`em_bus_event_type_…`) | Category | Replacement owner | Producers | Consumers | Other references |
|---|---|---|---|---|---|
| `none` | E | not ported | cmd/em_cmd.cpp | - | cmd/em_cmd.cpp |
| `chirp` | E | not ported | - | - | cmd/em_cmd.cpp |
| `reset` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, cmd/em_cmd.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_cmd_ctrl.cpp, ctrl/em_ctrl.cpp, em/em_mgr.cpp | cmd/em_cmd.cpp |
| `dev_test` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, cmd/em_cmd.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `set_dev_test` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `get_network` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `get_device` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `remove_device` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `get_radio` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `get_ssid` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `set_ssid` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, cmd/em_cmd.cpp, ctrl/dm_easy_mesh_ctrl.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `get_channel` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `set_channel` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `scan_channel` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, ctrl/dm_easy_mesh_ctrl.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | - |
| `scan_result` | A+B | em-cmd (`Request`) + em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp, cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | agent/em_agent.cpp, cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | - |
| `channel_select` | A | em-cmd (`Request`) | ctrl/dm_easy_mesh_ctrl.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `get_bss` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `get_sta` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `steer_sta` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, ctrl/dm_easy_mesh_ctrl.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `disassoc_sta` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, ctrl/dm_easy_mesh_ctrl.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `get_policy` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `set_policy` | A+C | em-cmd (`Request`) + em-core output action | cli/em_cmd_cli.cpp, ctrl/tr_181.cpp, em/em_policy_cfg.cpp, rdkb-cli/em_cmd_cli.cpp | agent/em_agent.cpp, cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `btm_sta` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `start_dpp` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `dev_init` | D | owning manager (em-ctrl / em-agent / em-orch) | agent/em_agent.cpp, cmd/em_cmd.cpp | agent/em_agent.cpp, cmd/em_cmd.cpp | cmd/em_cmd.cpp |
| `cfg_renew` | A+C | em-cmd (`Request`) + em-core output action | cli/em_cmd_cli.cpp, cmd/em_cmd.cpp, ctrl/em_ctrl.cpp, em/em_configuration.cpp, orch/em_orch_ctrl.cpp, rdkb-cli/em_cmd_cli.cpp | agent/em_agent.cpp, cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `radio_config` | E | not ported (no producer at this commit; only declarations, handlers and the `BUS_EVENT_TYPE_2S` string table) | - | agent/em_agent.cpp | cmd/em_cmd.cpp |
| `vap_config` | E | not ported (no producer at this commit; only declarations, handlers and the `BUS_EVENT_TYPE_2S` string table) | - | agent/em_agent.cpp | cmd/em_cmd.cpp |
| `sta_list` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp, cmd/em_cmd.cpp | agent/em_agent.cpp, cmd/em_cmd.cpp | cmd/em_cmd.cpp |
| `ap_cap_query` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | agent/em_agent.cpp, cmd/em_cmd.cpp | - |
| `client_cap_query` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp | - |
| `listener_stop` | D | owning manager (em-ctrl / em-agent / em-orch) | - | - | cmd/em_cmd.cpp |
| `dm_commit` | D | owning manager (em-ctrl / em-agent / em-orch) | ctrl/em_ctrl.cpp | ctrl/em_ctrl.cpp | cmd/em_cmd.cpp, em/em_mgr.cpp |
| `m2_tx` | C | em-core output action | em/em_configuration.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `topo_sync` | D | owning manager (em-ctrl / em-agent / em-orch) | cmd/em_cmd.cpp | cmd/em_cmd.cpp | cmd/em_cmd.cpp |
| `onewifi_private_cb` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp | - |
| `onewifi_mesh_sta_cb` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp | - |
| `onewifi_radio_cb` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp | - |
| `m2ctrl_configuration` | C | em-core output action | em/em_configuration.cpp | agent/em_agent.cpp | em/em_configuration.cpp |
| `sta_assoc` | C | em-core output action | em/em_configuration.cpp | ctrl/em_ctrl.cpp | - |
| `channel_pref_query` | C | em-core output action | em/em_channel.cpp | agent/em_agent.cpp | - |
| `channel_sel_req` | C | em-core output action | em/em_channel.cpp | agent/em_agent.cpp | em/em_channel.cpp |
| `sta_link_metrics` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp | - |
| `set_radio` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `bss_tm_req` | C | em-core output action | em/em_steering.cpp | agent/em_agent.cpp | - |
| `btm_response` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp | - |
| `channel_scan_params` | C | em-core output action | em/em_channel.cpp | agent/em_agent.cpp | - |
| `get_mld_config` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, cmd/em_cmd.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `mld_reconfig` | A | em-cmd (`Request`) | cli/em_cmd_cli.cpp, cmd/em_cmd.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `beacon_query` | A+C | em-cmd (`Request`) + em-core output action | ctrl/dm_easy_mesh_ctrl.cpp, em/em_metrics.cpp | agent/em_agent.cpp, cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `beacon_report` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp, cmd/em_cmd.cpp | - |
| `recv_wfa_action_frame` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp | - |
| `recv_gas_frame` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp | - |
| `get_sta_client_type` | E | not ported | - | - | - |
| `assoc_status` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp | - |
| `ap_metrics_report` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp, cmd/em_cmd.cpp | cmd/em_cmd.cpp |
| `bss_info` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp | - |
| `get_reset` | A | em-cmd (`Request`) | cmd/em_cmd.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_cmd_ctrl.cpp, ctrl/em_ctrl.cpp, em/em_mgr.cpp | cmd/em_cmd.cpp |
| `recv_csa_beacon_frame` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp | - |
| `bsta_cap_req` | C | em-core output action | em/em_configuration.cpp | ctrl/em_ctrl.cpp | - |
| `link_quality_report` | C | em-core output action (the OneWifi producer at `src/agent/em_agent.cpp:2162` is commented out) | em/em_metrics.cpp | agent/em_agent.cpp, cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp, agent/em_agent.cpp (commented-out producer, l.2162) |
| `client_assoc_ctrl_req` | A+C | em-cmd (`Request`) + em-core output action | cmd/em_cmd.cpp, ctrl/dm_easy_mesh_ctrl.cpp, em/em_steering.cpp, rdkb-cli/em_cmd_cli.cpp | agent/em_agent.cpp, cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `unassoc_sta_query` | A | em-cmd (`Request`) | cmd/em_cmd.cpp, ctrl/dm_easy_mesh_ctrl.cpp, rdkb-cli/em_cmd_cli.cpp | cmd/em_cmd.cpp, ctrl/em_ctrl.cpp | cmd/em_cmd.cpp |
| `unassoc_sta_link_metrics_query` | C | em-core output action | em/em_metrics.cpp | agent/em_agent.cpp | cmd/em_cmd.cpp |
| `unassoc_sta_result` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp, cmd/em_cmd.cpp | agent/em_agent.cpp, cmd/em_cmd.cpp | cmd/em_cmd.cpp |
| `failed_conn` | B | em-core input event; em-agent translates OneWifi payload | agent/em_agent.cpp | agent/em_agent.cpp | cmd/em_cmd.cpp |
| `vendor_data` | E | not ported (no producer at this commit; only declarations, handlers and the `BUS_EVENT_TYPE_2S` string table) | - | agent/em_agent.cpp, cmd/em_cmd.cpp | - |
| `sta_cap_update` | C | em-core output action | em/em_capability.cpp | ctrl/em_ctrl.cpp | - |
| `sta_mld_update` | C | em-core output action | em/em_configuration.cpp | ctrl/em_ctrl.cpp | - |
| `topo_publish` | D | owning manager (em-ctrl / em-agent / em-orch) | ctrl/em_ctrl.cpp | ctrl/em_ctrl.cpp | - |
| `max` | E | not ported | - | - | - |
