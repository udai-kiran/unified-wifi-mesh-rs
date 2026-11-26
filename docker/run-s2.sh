#!/bin/bash
# Builds the harness, starts two nodes in HARNESS_MODE=s2 and runs the gating exit checks of docs/0013 D10
# (1 to 4 and 6) with the real binaries: em-agent (node 1, radios 2.4 GHz and 5 GHz) and em-ctrl (node 2,
# reset file /nvram/Reset.json) over the real ieee1905-rs.
#
#   docker/run-s2.sh              gating run: checks 1, 2, 3 and 6 on a normal run, check 4 on a second run
#                                 with em-ctrl --test-drop-first-m2 (the first M2 is never sent)
#   docker/run-s2.sh --eviction   check 5: the lost M2 run with a 70 s block of the controller's egress for 1905
#                                 frames; prints the timeline and whether D10's prediction held (evidence only),
#                                 and gates on check 5r: exactly 5 "renew sent" per radio in the block, none after
#
# The gating run prints "PASS <n> <name> (<evidence>)" or "FAIL <n> <name>: <reason>" per check, in order, at
# the end, and exits non-zero on any failure. KEEP=1 leaves the containers running afterwards. Both modes also
# print the D9 stall times (build_us / check_us fields of the role logs).
#
# Every search of a log starts after a line number or a timestamp that the event under test follows, and a
# check that reads zero matching lines fails: no check passes because nothing was logged. Check 7 of D10
# (workspace checks) is cargo, not this script.
set -u

cd "$(dirname "$0")/.." || exit 2
COMPOSE=(docker compose -f docker/compose.yaml)
export HARNESS_MODE=s2

case "${1:-}" in
"") MODE=gating ;;
--eviction) MODE=eviction ;;
*)
    echo "usage: docker/run-s2.sh [--eviction]" >&2
    exit 2
    ;;
esac

NODE1=em-s0-node1 # agent
NODE2=em-s0-node2 # controller
NODE1_MAC=02:42:c0:a8:65:02
NODE2_MAC=02:42:c0:a8:65:03
RADIO_24=02:00:00:00:01:01
RADIO_5=02:00:00:00:01:02
AGENT_RADIOS="--radio $RADIO_24,0 --radio $RADIO_5,1"
RESET_FILE=/nvram/Reset.json
AGENT_LOG=em-agent.log
CTRL_LOG=em-ctrl.log
LOG_DIR=/var/log/s0
IFACE=eth0

REGISTER_WAIT_S=60
READY_WAIT_S=30
ONBOARD_WAIT_S=150 # from registration to both radios topo_synchronized (discovery can take 30 s)
# D10 1 to 3 and D9.
M2_LIMIT_S=1       # each M2 within 1 s of its M1 (EasyMesh 7.1)
RESPONSE_LIMIT_S=5 # Topology Response within 5 s of the last radio reaching onewifi_bssconfig_ind
# The Docker reset file has a Fronthaul and a Backhaul row for both bands, so each radio gets two BSSs.
BSS_PER_RADIO=2
# D10 4: the radio restarts discovery when more than 10 s have passed on a 1 s proto tick, so 11 s to
# about 12.5 s after its M1; 14 s is the limit of what the check accepts.
RESTART_MIN_S=10
RESTART_MAX_S=14
# D10 6: after the new session, anything the role processes did would show within a few 1 s ticks.
SESSION_WINDOW_S=10
# D10 5: the block of the controller's egress.
BLOCK_S=70
EVICTION_SETTLE_S=240 # how long after the block lifts the run waits for both radios to settle

declare -A CHECK_NAME=(
    [1]="M1 after its Response, new MID, MAC Address = agent AL MAC"
    [2]="M2 within 1 s, new MID, accepted, BSSs of the matching rows recorded"
    [3]="one confirmation, Topology Response within 5 s, both radios topo_synchronized"
    [4]="lost M2: restart after more than 10 s, both topo_synchronized, one dev_init"
    [6]="ieee1905-rs restarts keep the role processes and their state"
)
declare -A CHECK_EVIDENCE CHECK_FAILURES
failures=0

WORK=$(mktemp -d)
SCAN=""
D9_LINES=""

teardown() {
    rm -rf "$WORK"
    if [ "${KEEP:-0}" = 1 ]; then
        echo "KEEP=1: containers left running (HARNESS_MODE=s2 docker compose -f docker/compose.yaml down to remove)"
    else
        "${COMPOSE[@]}" down --timeout 5 >/dev/null 2>&1
    fi
}
trap teardown EXIT

# note <check> <text>: evidence printed with the PASS line. flag <check> <text>: a failure reason.
note() { CHECK_EVIDENCE[$1]+="${CHECK_EVIDENCE[$1]:+; }$2"; }
flag() { CHECK_FAILURES[$1]+="${CHECK_FAILURES[$1]:+; }$2"; }

report_all() {
    local n
    for n in 1 2 3 4 6; do
        if [ -n "${CHECK_FAILURES[$n]:-}" ]; then
            echo "FAIL $n ${CHECK_NAME[$n]}: ${CHECK_FAILURES[$n]}"
            failures=$((failures + 1))
        elif [ -z "${CHECK_EVIDENCE[$n]:-}" ]; then
            echo "FAIL $n ${CHECK_NAME[$n]}: not run (an earlier step failed)"
            failures=$((failures + 1))
        else
            echo "PASS $n ${CHECK_NAME[$n]} (${CHECK_EVIDENCE[$n]})"
        fi
    done
}

# read_log <node> <file>: the file's content; the status is that of docker exec, so a failed read is
# never taken for an empty log. Every helper below passes that status on, and a check that asserts
# that something did not happen flags itself when the log could not be read.
read_log() { docker exec "$1" cat "$LOG_DIR/$2"; }
# snapshot <node> <file> <destination>: one read of a role log into a local file.
snapshot() { read_log "$1" "$2" >"$3"; }
total_lines() {
    local snap=$WORK/total.log
    snapshot "$1" "$2" "$snap" || return 1
    wc -l <"$snap"
}
now_epoch() { date +%s.%N; }
lines_in() { printf '%s\n' "$1" | grep -c .; }
first_of() { printf '%s\n' "$1" | head -n 1; }
# field <name>: the value of name=value in each log line on stdin.
field() { sed -n "s/.* $1=\([^ ]*\).*/\1/p"; }
# Floating point comparisons: fgt a b is a > b, fge is a >= b.
fgt() { awk -v a="$1" -v b="$2" 'BEGIN { exit !(a > b) }'; }
fge() { awk -v a="$1" -v b="$2" 'BEGIN { exit !(a >= b) }'; }
fsum() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.3f", a + b }'; }
fdiff() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.1f", a - b }'; }
sleep_until() {
    local delay
    delay=$(awk -v target="$1" -v now="$(now_epoch)" 'BEGIN { d = target - now; print (d > 0 ? d : 0) }')
    sleep "$delay"
}

# scan_snapshot <snapshot file> <fixed string> <lines to skip> <last line, 0 for the end>
# One output line per matching line after the skipped ones: "<epoch> <line number> <the log line>".
# The epoch is the line's own timestamp (tracing prints UTC), so times come from the logs, not from
# when this script looked. Fails if any step fails.
scan_snapshot() {
    local raw=$WORK/scan.raw times=$WORK/scan.times epochs=$WORK/scan.epochs
    awk -v skip="$3" -v last="$4" -v needle="$2" \
        'NR > skip && (last == 0 || NR <= last) && index($0, needle) { print NR, $0 }' "$1" >"$raw" || return 1
    awk '{ print $2 }' "$raw" >"$times" || return 1
    date -u -f "$times" +%s.%N >"$epochs" || return 1
    paste -d' ' "$epochs" "$raw"
}

# scan <node> <file> <fixed string> <lines to skip>
# scan_snapshot on a fresh read of the log; fails if the log cannot be read.
scan() {
    local snap=$WORK/scan.log
    snapshot "$1" "$2" "$snap" || return 1
    scan_snapshot "$snap" "$3" "$4" 0
}

# wait_scan <node> <file> <fixed string> <lines to skip> <min matches> <timeout s>
# Succeeds when at least <min matches> lines match; the matches are left in SCAN.
wait_scan() {
    local waited=0
    while true; do
        # A failed read counts as no match; the wait then ends in failure at the timeout.
        if SCAN=$(scan "$1" "$2" "$3" "$4") && [ "$(lines_in "$SCAN")" -ge "$5" ]; then
            return 0
        fi
        if [ "$waited" -ge "$6" ]; then
            return 1
        fi
        sleep 1
        waited=$((waited + 1))
    done
}

# expect_none <check> <node> <file> <lines to skip> <last line, 0 for the end> <description> <needle>...
# Flags the check if any needle matches a line after the skipped ones (up to the last line). The log
# is read once and every needle is judged against that snapshot. A log that cannot be read, or a scan
# that fails, flags the check: the absence was not verified.
expect_none() {
    local check=$1 node=$2 file=$3 skip=$4 last=$5 description=$6 needle matches
    local snap=$WORK/none.log scope="after line $4"
    shift 6
    if [ "$last" -ne 0 ]; then
        scope="$scope up to line $last"
    fi
    if ! snapshot "$node" "$file" "$snap"; then
        flag "$check" "$description: could not read $file in $node, so the absence checks were not verified"
        return
    fi
    for needle in "$@"; do
        if ! matches=$(scan_snapshot "$snap" "$needle" "$skip" "$last"); then
            flag "$check" "$description: scanning $file in $node for '$needle' failed, so its absence was not verified"
        elif [ -n "$matches" ]; then
            flag "$check" "$description: '$needle' logged in $file $scope: $(first_of "$matches" | cut -d' ' -f3-)"
        fi
    done
}

# ---- Small helpers on scan output ("<epoch> <line number> <log line>")
epoch_of() { cut -d' ' -f1; }
line_of() { cut -d' ' -f2; }
# for_radio <scan output> <radio>: the lines of one radio (radio=<mac> is sometimes the last field).
for_radio() { printf '%s\n' "$1" | grep -E "radio=$2( |\$)"; }
# A <needle>, C <needle>: matching lines of the agent / controller snapshot taken by snapshot_both.
A() { scan_snapshot "$WORK/agent.snap" "$1" 0 0; }
C() { scan_snapshot "$WORK/ctrl.snap" "$1" 0 0; }
snapshot_both() {
    snapshot "$NODE1" "$AGENT_LOG" "$WORK/agent.snap" && snapshot "$NODE2" "$CTRL_LOG" "$WORK/ctrl.snap"
}

# D9: the actor's stall for each handler that does the cryptography, from the log fields.
# collect_d9 reads the snapshots; print_d9 prints the longest of each over every run of this script.
collect_d9() {
    A "M1 sent" | field build_us >>"$WORK/d9.agent_m1"
    A "M2 accepted" | field check_us >>"$WORK/d9.agent_m2"
    C "M2 sent" | field build_us >>"$WORK/d9.ctrl_m2"
}
d9_line() { # <label> <file>
    if [ -s "$2" ]; then
        sort -n "$2" | awk -v label="$1" '{ v[NR] = $1 } END { printf "INFO D9 %s: %d samples, longest %d us, median %d us\n", label, NR, v[NR], v[int((NR + 1) / 2)] }'
    else
        echo "INFO D9 $1: no samples"
    fi
}
print_d9() {
    d9_line "agent M1 build (DH key pair, 1536-bit MODP)" "$WORK/d9.agent_m1"
    d9_line "agent M2 check (shared secret, KDF, AES-CBC decrypt, per M2)" "$WORK/d9.agent_m2"
    d9_line "controller M2 build (DH key pair, shared secret, KDF, AES-CBC encrypt, all M2s of one M1)" "$WORK/d9.ctrl_m2"
}

dump_logs() {
    local node file
    for node in "$NODE1" "$NODE2"; do
        for file in "$AGENT_LOG" "$CTRL_LOG" ieee1905.log supervisor.log; do
            echo "--- $node $file (last 30 lines)"
            read_log "$node" "$file" 2>&1 | tail -n 30
        done
    done
}

finish() {
    report_all
    print_d9
    if [ "$failures" -ne 0 ]; then
        dump_logs
        echo "$failures check(s) failed"
        exit 1
    fi
    echo "All checks passed"
    exit 0
}

# bail <reason>: a step every later check depends on failed.
bail() {
    echo "ABORT: $1"
    if [ "$MODE" = eviction ]; then
        dump_logs
        exit 1
    fi
    finish
}

# start_nodes <check> <extra em-ctrl arguments>
# Builds and starts both nodes, then em-ctrl and em-agent, and waits until both have registered.
# Leaves CTRL_REG_LINE and AGENT_REG_LINE (the log lines of the "registered" messages). A failure is
# flagged on <check> and aborts.
start_nodes() {
    local check=$1 ctrl_args=$2 node waited
    echo "Building and starting the harness (HARNESS_MODE=s2)"
    if ! "${COMPOSE[@]}" up --build -d --force-recreate; then
        flag "$check" "docker compose up failed"
        bail "docker compose up failed"
    fi
    for node in "$NODE1" "$NODE2"; do
        waited=0
        until docker exec "$node" test -e "$LOG_DIR/role.ready"; do
            if [ "$waited" -ge "$READY_WAIT_S" ]; then
                flag "$check" "$node did not write $LOG_DIR/role.ready within ${READY_WAIT_S}s"
                bail "the supervisor in $node is not ready"
            fi
            sleep 1
            waited=$((waited + 1))
        done
    done
    # shellcheck disable=SC2086 # ctrl_args is zero or one word
    if ! docker exec "$NODE2" role-ctl start --reset-file "$RESET_FILE" $ctrl_args; then
        flag "$check" "role-ctl start failed in $NODE2"
        bail "cannot start em-ctrl"
    fi
    if ! wait_scan "$NODE2" "$CTRL_LOG" "em_ctrl::actor: registered session=" 0 1 "$REGISTER_WAIT_S"; then
        flag "$check" "em-ctrl did not log 'registered' within ${REGISTER_WAIT_S}s"
        bail "em-ctrl did not register"
    fi
    CTRL_REG_LINE=$(first_of "$SCAN" | line_of)
    # shellcheck disable=SC2086 # AGENT_RADIOS is several words
    if ! docker exec "$NODE1" role-ctl start $AGENT_RADIOS --profile 3; then
        flag "$check" "role-ctl start failed in $NODE1"
        bail "cannot start em-agent"
    fi
    if ! wait_scan "$NODE1" "$AGENT_LOG" "em_agent::actor: registered session=" 0 1 "$REGISTER_WAIT_S"; then
        flag "$check" "em-agent did not log 'registered' within ${REGISTER_WAIT_S}s"
        bail "em-agent did not register"
    fi
    AGENT_REG_LINE=$(first_of "$SCAN" | line_of)
}

# wait_both_synchronized <check>
# Waits until both radios have logged topo_synchronized on the agent and on the controller.
wait_both_synchronized() {
    if ! wait_scan "$NODE1" "$AGENT_LOG" "topo_synchronized radio=" "$AGENT_REG_LINE" 2 "$ONBOARD_WAIT_S"; then
        flag "$1" "fewer than 2 'topo_synchronized radio=' lines in $AGENT_LOG within ${ONBOARD_WAIT_S}s"
        bail "the agent's radios did not reach topo_synchronized"
    fi
    if ! wait_scan "$NODE2" "$CTRL_LOG" "topo_synchronized radio=" "$CTRL_REG_LINE" 2 20; then
        flag "$1" "fewer than 2 'topo_synchronized radio=' lines in $CTRL_LOG within 20s of the agent's"
        bail "the controller's radio nodes did not reach topo_synchronized"
    fi
}

# ---- Checks 1 to 3, on the snapshots of a normal run
declare -A M1_EPOCH M1_MID M1_LINE ACCEPT_LINE APPLY_LINE
check_normal_run() {
    local radio found_lines found_line found_no m1_lines m1 m1_mid m1_mac mids created ignored
    local m2_lines m2 m2_mid m2_epoch m2_count accepted accepted_mid accepted_bss applied applied_bss gap
    local radios_checked_1=0 radios_checked_2=0
    if ! snapshot_both; then
        flag 1 "could not read the role logs"
        flag 2 "could not read the role logs"
        flag 3 "could not read the role logs"
        return
    fi
    collect_d9
    # Every MID this run used before M1: Searches and Responses.
    mids=$( (A "Search sent radio=" | field mid; C "Response sent" | field mid) | sort -u)
    created=$(C "radio node created")
    for radio in "$RADIO_24" "$RADIO_5"; do
        # ---- Check 1
        found_lines=$(for_radio "$(A "radio found controller")" "$radio")
        m1_lines=$(for_radio "$(A "M1 sent")" "$radio")
        if [ "$(lines_in "$found_lines")" -ne 1 ] || [ "$(lines_in "$m1_lines")" -ne 1 ]; then
            flag 1 "radio $radio has $(lines_in "$found_lines") 'radio found controller' and $(lines_in "$m1_lines") 'M1 sent' lines, expected 1 each"
            continue
        fi
        found_no=$(echo "$found_lines" | line_of)
        m1=$m1_lines
        m1_mid=$(echo "$m1" | field mid)
        m1_mac=$(echo "$m1" | field mac)
        M1_EPOCH[$radio]=$(echo "$m1" | epoch_of)
        M1_MID[$radio]=$m1_mid
        M1_LINE[$radio]=$(echo "$m1" | line_of)
        if [ "$(echo "$m1" | line_of)" -le "$found_no" ]; then
            flag 1 "radio $radio: M1 (log line $(echo "$m1" | line_of)) is not after its 'radio found controller' (line $found_no)"
        fi
        if [ -z "$m1_mid" ] || printf '%s\n' "$mids" | grep -qx "$m1_mid"; then
            flag 1 "radio $radio: M1 MID '$m1_mid' is empty or was used by a Search or Response before"
        fi
        if [ "$m1_mac" != "$NODE1_MAC" ]; then
            flag 1 "radio $radio: M1 MAC Address is '$m1_mac', expected the agent AL MAC $NODE1_MAC"
        fi
        if [ "$(lines_in "$(for_radio "$created" "$radio" | grep -F "agent=$NODE1_MAC")")" -ne 1 ]; then
            flag 1 "radio $radio: the controller did not log exactly one 'radio node created' for agent $NODE1_MAC"
        fi
        radios_checked_1=$((radios_checked_1 + 1))
        note 1 "radio $radio: M1 mid=$m1_mid mac=$m1_mac at agent log line ${M1_LINE[$radio]}, after its Response (line $found_no), MID not used before"

        # ---- Check 2
        m2_lines=$(for_radio "$(C "M2 sent")" "$radio")
        if [ "$(lines_in "$m2_lines")" -ne 1 ]; then
            flag 2 "radio $radio: the controller logged $(lines_in "$m2_lines") 'M2 sent' lines, expected 1"
            continue
        fi
        m2=$m2_lines
        m2_mid=$(echo "$m2" | field mid)
        m2_epoch=$(echo "$m2" | epoch_of)
        m2_count=$(echo "$m2" | field m2s)
        gap=$(fdiff "$m2_epoch" "${M1_EPOCH[$radio]}")
        if ! fgt "$m2_epoch" "${M1_EPOCH[$radio]}" || fgt "$gap" "$M2_LIMIT_S"; then
            flag 2 "radio $radio: M2 sent ${gap}s after the M1 in the logs, limit ${M2_LIMIT_S}s (and not before it)"
        fi
        if [ -z "$m2_mid" ] || [ "$m2_mid" = "$m1_mid" ]; then
            flag 2 "radio $radio: M2 MID '$m2_mid' is empty or equals the M1 MID"
        fi
        if [ "$m2_count" != "$BSS_PER_RADIO" ]; then
            flag 2 "radio $radio: the controller sent $m2_count M2s in the CMDU, expected $BSS_PER_RADIO (Fronthaul and Backhaul rows)"
        fi
        accepted=$(for_radio "$(A "M2 accepted")" "$radio")
        applied=$(for_radio "$(A "platform applied a radio configuration")" "$radio")
        if [ "$(lines_in "$accepted")" -ne 1 ]; then
            flag 2 "radio $radio: the agent logged $(lines_in "$accepted") 'M2 accepted' lines, expected 1"
        else
            accepted_mid=$(echo "$accepted" | field mid)
            accepted_bss=$(echo "$accepted" | field bss)
            ACCEPT_LINE[$radio]=$(echo "$accepted" | line_of)
            if [ "$accepted_mid" != "$m2_mid" ]; then
                flag 2 "radio $radio: the agent accepted M2 MID '$accepted_mid', the controller sent '$m2_mid'"
            fi
            if [ "$accepted_bss" != "$BSS_PER_RADIO" ]; then
                flag 2 "radio $radio: the agent accepted $accepted_bss BSS, expected $BSS_PER_RADIO"
            fi
        fi
        if [ "$(lines_in "$applied")" -ne 1 ]; then
            flag 2 "radio $radio: the fake platform logged $(lines_in "$applied") 'platform applied a radio configuration' lines, expected 1"
        else
            applied_bss=$(echo "$applied" | field bss)
            APPLY_LINE[$radio]=$(echo "$applied" | line_of)
            if [ "$applied_bss" != "$BSS_PER_RADIO" ]; then
                flag 2 "radio $radio: the fake platform recorded $applied_bss BSS, expected $BSS_PER_RADIO"
            elif [ -n "${ACCEPT_LINE[$radio]:-}" ] && [ "${APPLY_LINE[$radio]}" -le "${ACCEPT_LINE[$radio]}" ]; then
                flag 2 "radio $radio: the platform applied the configuration before the agent accepted the M2"
            fi
        fi
        radios_checked_2=$((radios_checked_2 + 1))
        note 2 "radio $radio: M2 mid=$m2_mid (M1 mid=$m1_mid) ${gap}s after M1, $m2_count M2s, accepted bss=${accepted_bss:-?}, platform recorded bss=${applied_bss:-?}"
    done
    # A radio that was skipped above must fail the check, not pass on the other radio's evidence.
    if [ "$radios_checked_1" -ne 2 ]; then
        flag 1 "only $radios_checked_1 of 2 radios were checked"
    fi
    if [ "$radios_checked_2" -ne 2 ]; then
        flag 2 "only $radios_checked_2 of 2 radios were checked"
    fi
    expect_none 2 "$NODE1" "$AGENT_LOG" 0 0 "agent" "M2 rejected" "Response rejected"
    expect_none 2 "$NODE2" "$CTRL_LOG" 0 0 "controller" "M2 dropped by test switch" "M2 could not be built" "M2 failed validation" "M1 rejected" "M1 dropped"
    check_confirmation
}

# Check 3, on the snapshots.
check_confirmation() {
    local confirmed callbacks cb cb_epoch cb_line answered answered_first before handled handled_epoch delay radio lines queries early
    confirmed=$(A "platform confirmed")
    callbacks=$(A "onewifi_cb submitted")
    if [ "$(lines_in "$confirmed")" -ne 1 ] || [ "$(lines_in "$callbacks")" -ne 1 ]; then
        flag 3 "the agent logged $(lines_in "$confirmed") 'platform confirmed' and $(lines_in "$callbacks") 'onewifi_cb submitted' lines, expected 1 each"
        return
    fi
    cb=$callbacks
    cb_epoch=$(echo "$cb" | epoch_of)
    cb_line=$(echo "$cb" | line_of)
    if [ "$(echo "$confirmed" | field bss)" != "$((2 * BSS_PER_RADIO))" ]; then
        flag 3 "the platform confirmed $(echo "$confirmed" | field bss) BSS, expected $((2 * BSS_PER_RADIO))"
    fi
    for radio in "$RADIO_24" "$RADIO_5"; do
        if [ -z "${APPLY_LINE[$radio]:-}" ] || [ "${APPLY_LINE[$radio]}" -ge "$(echo "$confirmed" | line_of)" ]; then
            flag 3 "radio $radio: no platform configuration before the confirmation"
        fi
    done
    if [ "$(echo "$confirmed" | line_of)" -ge "$cb_line" ]; then
        flag 3 "the confirmation is not before 'onewifi_cb submitted'"
    fi
    # onewifi_cb moves every radio to onewifi_bssconfig_ind when it starts, and the agent answers a Query only
    # when every radio is at least there, so the first answer proves both radios reached it.
    answered=$(A "Topology Query answered")
    if [ "$(lines_in "$answered")" -lt 1 ]; then
        flag 3 "the agent answered no Topology Query"
        return
    fi
    answered_first=$(first_of "$answered")
    early=$(printf '%s\n' "$answered" | awk -v line="$cb_line" '$2 < line' | grep -c .)
    if [ "$early" -ne 0 ]; then
        flag 3 "the agent answered a Topology Query before onewifi_cb was submitted"
    fi
    queries=$(C "Topology Query sent")
    before=$(printf '%s\n' "$queries" | awk -v t="$cb_epoch" '$1 < t' | grep -c .)
    handled=$(C "Topology Response handled")
    if [ "$(lines_in "$handled")" -lt 1 ]; then
        flag 3 "the controller logged no 'Topology Response handled'"
        return
    fi
    handled_epoch=$(first_of "$handled" | epoch_of)
    delay=$(fdiff "$handled_epoch" "$cb_epoch")
    if ! fge "$handled_epoch" "$cb_epoch" || fgt "$delay" "$RESPONSE_LIMIT_S"; then
        flag 3 "the Topology Response was handled ${delay}s after onewifi_cb was submitted, limit ${RESPONSE_LIMIT_S}s (and not before it)"
    fi
    for radio in "$RADIO_24" "$RADIO_5"; do
        lines=$(for_radio "$(A "topo_synchronized radio=")" "$radio" | awk -v line="$(echo "$answered_first" | line_of)" '$2 > line')
        if [ "$(lines_in "$lines")" -lt 1 ]; then
            flag 3 "radio $radio: no agent 'topo_synchronized' after the Topology Query was answered"
        fi
        lines=$(for_radio "$(C "topo_synchronized radio=")" "$radio" | awk -v t="$handled_epoch" '$1 >= t')
        if [ "$(lines_in "$lines")" -lt 1 ]; then
            flag 3 "radio $radio: no controller 'topo_synchronized' after the Topology Response was handled"
        fi
    done
    expect_none 3 "$NODE1" "$AGENT_LOG" 0 0 "agent" "restart discovery" "onewifi_cb cancelled" "dev_init cancelled"
    expect_none 3 "$NODE2" "$CTRL_LOG" 0 0 "controller" "ssid mismatch" "em_config cancelled" "renew sent" "Topology Response rejected"
    note 3 "one 'platform confirmed' (bss=$(echo "$confirmed" | field bss)) and one 'onewifi_cb submitted' (line $cb_line); $before Queries sent before it went unanswered; Topology Response handled ${delay}s after it (limit ${RESPONSE_LIMIT_S}s); both radios topo_synchronized on the agent and the controller"
}


# ---- Check 6: ieee1905-rs killed on either node after configuration
# After the baseline taken before the kill, neither role process may start, search, onboard or restart
# anything: the role processes reconnect and keep their state (both radios stay topo_synchronized).
AGENT_QUIET=("Search sent" "radio found controller" "controller found controller=" "dev_init submitted"
    "dev_init has no candidates" "em-agent starting" "Response rejected" "M1 sent" "M2 accepted" "M2 rejected"
    "platform confirmed" "onewifi_cb submitted" "onewifi_cb cancelled" "dev_init cancelled" "restart discovery")
CTRL_QUIET=("peer model created" "Response sent" "em-ctrl starting" "radio node created" "M2 sent"
    "M2 dropped by test switch" "em_config cancelled" "ssid mismatch" "renew sent")

# service_restart_check <node> <role log of the node> <AL MAC of the node> <label>
service_restart_check() {
    local node=$1 role_log=$2 mac=$3 label=$4
    local agent_skip ctrl_skip agent_pid ctrl_pid service_pid_before service_pid_after
    local kill_epoch session session_line session_epoch session_mac role_skip agent_pid_after ctrl_pid_after
    if ! agent_skip=$(total_lines "$NODE1" "$AGENT_LOG") || ! ctrl_skip=$(total_lines "$NODE2" "$CTRL_LOG"); then
        flag 6 "$label: could not read the role logs for the baseline"
        return
    fi
    # A role pid or the service pid that cannot be read must not compare equal to a later failed read.
    if ! agent_pid=$(docker exec "$NODE1" role-ctl pid) || ! ctrl_pid=$(docker exec "$NODE2" role-ctl pid) ||
        ! service_pid_before=$(docker exec "$node" cat "$LOG_DIR/ieee1905.pid"); then
        flag 6 "$label: could not read the role or ieee1905 pids before the kill"
        return
    fi
    kill_epoch=$(now_epoch)
    if ! docker exec "$node" bash -c "kill \$(cat $LOG_DIR/ieee1905.pid)"; then
        flag 6 "$label: could not kill ieee1905 in $node"
        return
    fi
    role_skip=$ctrl_skip
    if [ "$node" = "$NODE1" ]; then
        role_skip=$agent_skip
    fi
    if ! wait_scan "$node" "$role_log" "new session, same AL MAC" "$role_skip" 1 "$REGISTER_WAIT_S"; then
        flag 6 "$label: no 'new session, same AL MAC' in $role_log of $node within ${REGISTER_WAIT_S}s of the kill"
        return
    fi
    session=$(first_of "$SCAN")
    session_epoch=$(echo "$session" | epoch_of)
    session_line=$(echo "$session" | line_of)
    session_mac=$(echo "$session" | field al_mac)
    if [ "$session_mac" != "$mac" ]; then
        flag 6 "$label: the new session has AL MAC '$session_mac', expected $mac"
    fi
    if ! fgt "$session_epoch" "$kill_epoch"; then
        flag 6 "$label: the new session line is not after the kill"
    fi
    sleep_until "$(fsum "$session_epoch" "$SESSION_WINDOW_S")"
    if ! service_pid_after=$(docker exec "$node" cat "$LOG_DIR/ieee1905.pid"); then
        flag 6 "$label: could not read the ieee1905 pid after the kill"
    elif [ "$service_pid_after" = "$service_pid_before" ]; then
        flag 6 "$label: ieee1905 kept pid $service_pid_before, so it was not restarted"
    fi
    # Both role logs are searched after the baseline taken before the kill, not after the new-session line.
    expect_none 6 "$NODE1" "$AGENT_LOG" "$agent_skip" 0 "$label" "${AGENT_QUIET[@]}"
    expect_none 6 "$NODE2" "$CTRL_LOG" "$ctrl_skip" 0 "$label" "${CTRL_QUIET[@]}"
    if ! agent_pid_after=$(docker exec "$NODE1" role-ctl pid) || ! ctrl_pid_after=$(docker exec "$NODE2" role-ctl pid); then
        flag 6 "$label: could not read the role pids after the kill"
    elif [ "$agent_pid_after" != "$agent_pid" ] || [ "$ctrl_pid_after" != "$ctrl_pid" ]; then
        flag 6 "$label: a role process was restarted (its pid changed)"
    fi
    note 6 "$label: new session at line $session_line with AL MAC $mac; after the pre-kill baseline (agent log line $agent_skip, controller log line $ctrl_skip) no Search, M1, M2, dev_init, platform confirmation, restart discovery, em_config cancel, renew or process start; role pids unchanged; ieee1905 pid changed; waited ${SESSION_WINDOW_S}s after the new session"
}

# ---- Check 4: the lost-M2 run, on the snapshots after both radios synchronised
check_lost_m2() {
    local dropped dropped_line drop_radio m2_after restarts first_restart restart_epoch m1_first gap radio lines
    local submitted resets
    if ! snapshot_both; then
        flag 4 "could not read the role logs"
        return
    fi
    collect_d9
    dropped=$(C "M2 dropped by test switch")
    if [ "$(lines_in "$dropped")" -ne 1 ]; then
        flag 4 "the controller logged $(lines_in "$dropped") 'M2 dropped by test switch' lines, expected 1"
        return
    fi
    dropped_line=$(echo "$dropped" | line_of)
    # The dropped M2 is logged as sent on the next line; its radio is the one that must restart.
    m2_after=$(C "M2 sent" | awk -v line="$dropped_line" '$2 > line' | head -n 1)
    drop_radio=$(echo "$m2_after" | field radio)
    if [ -z "$drop_radio" ]; then
        flag 4 "no 'M2 sent' follows the dropped M2 in the controller log"
        return
    fi
    restarts=$(A "restart discovery" | grep -F "no M2 within 10 s of M1")
    if [ "$(lines_in "$restarts")" -lt 1 ]; then
        flag 4 "the agent never logged 'restart discovery' with the 10 s M2 reason"
        return
    fi
    first_restart=$(first_of "$restarts")
    restart_epoch=$(echo "$first_restart" | epoch_of)
    if [ "$(echo "$first_restart" | field radio)" != "$drop_radio" ]; then
        flag 4 "the first restart is for radio '$(echo "$first_restart" | field radio)', the dropped M2 was for $drop_radio"
    fi
    m1_first=$(for_radio "$(A "M1 sent")" "$drop_radio" | head -n 1)
    if [ -z "$m1_first" ]; then
        flag 4 "no 'M1 sent' for radio $drop_radio"
        return
    fi
    gap=$(fdiff "$restart_epoch" "$(echo "$m1_first" | epoch_of)")
    if ! fgt "$gap" "$RESTART_MIN_S" || fgt "$gap" "$RESTART_MAX_S"; then
        flag 4 "radio $drop_radio restarted discovery ${gap}s after its first M1, expected more than ${RESTART_MIN_S}s and at most ${RESTART_MAX_S}s"
    fi
    lines=$(for_radio "$(A "M1 sent")" "$drop_radio")
    if [ "$(lines_in "$lines")" -ne 2 ]; then
        flag 4 "radio $drop_radio sent $(lines_in "$lines") M1s, expected 2 (the first, then one after the restart)"
    fi
    accepted=$(for_radio "$(A "M2 accepted")" "$drop_radio")
    if [ "$(lines_in "$accepted")" -ne 1 ] || [ "$(echo "$accepted" | line_of)" -le "$(echo "$first_restart" | line_of)" ]; then
        flag 4 "radio $drop_radio: expected exactly one 'M2 accepted', after the restart; got: $accepted"
    fi
    for radio in "$RADIO_24" "$RADIO_5"; do
        lines=$(for_radio "$(A "topo_synchronized radio=")" "$radio" | awk -v t="$restart_epoch" '$1 > t')
        if [ "$(lines_in "$lines")" -lt 1 ]; then
            flag 4 "radio $radio: no agent 'topo_synchronized' after the restart"
        fi
        lines=$(for_radio "$(C "topo_synchronized radio=")" "$radio" | awk -v t="$restart_epoch" '$1 > t')
        if [ "$(lines_in "$lines")" -lt 1 ]; then
            flag 4 "radio $radio: no controller 'topo_synchronized' after the restart"
        fi
    done
    submitted=$(A "dev_init submitted")
    if [ "$(lines_in "$submitted")" -ne 1 ]; then
        flag 4 "the agent logged $(lines_in "$submitted") 'dev_init submitted' lines, expected exactly 1"
    fi
    resets=$(A "restart discovery" | grep -vF "no M2 within 10 s of M1")
    if [ "$(lines_in "$resets")" -ne 0 ]; then
        flag 4 "the agent restarted discovery for another reason: $(first_of "$resets" | cut -d' ' -f3-)"
    fi
    expect_none 4 "$NODE1" "$AGENT_LOG" 0 0 "agent" "M2 rejected" "onewifi_cb cancelled" "dev_init cancelled"
    expect_none 4 "$NODE2" "$CTRL_LOG" 0 0 "controller" "em_config cancelled" "ssid mismatch" "renew sent"
    note 4 "controller dropped the M2 for radio $drop_radio (line $dropped_line); that radio restarted discovery ${gap}s after its M1, sent a second M1 and had its M2 accepted; both radios topo_synchronized on the agent and the controller; exactly one 'dev_init submitted'"
}


# ---- Check 5: the eviction run (evidence, not gating)
# timeline_source <label> <snapshot> <first line to read> <regex>: "<label> <epoch> <log line>" for the
# matching lines; fails if the timestamps cannot be converted.
timeline_source() {
    local raw=$WORK/tl.raw times=$WORK/tl.times epochs=$WORK/tl.epochs
    awk -v skip="$3" 'NR > skip' "$2" | grep -E "$4" >"$raw"
    awk '{ print $1 }' "$raw" >"$times"
    date -u -f "$times" +%s.%N >"$epochs" || return 1
    paste -d' ' "$epochs" "$raw" | sed "s/^/$1 /"
}

AGENT_TIMELINE='M1 sent|M2 accepted|M2 rejected|platform confirmed|onewifi_cb submitted|onewifi_cb cancelled|Topology Query answered|topo_synchronized radio=|restart discovery|dev_init submitted|dev_init cancelled|renew accepted|renew refused|renew ignored|radio found controller|a radio is unconfigured and no dev_init is active|Search sent'
CTRL_TIMELINE='radio node created|M2 sent|M2 dropped|M1 ignored|Topology Query sent|Topology Response handled|topo_synchronized radio=|ssid mismatch|em_config cancelled|renew sent|peer model created|peer model evicted|Topology Response ignored'
SERVICE_TIMELINE='Removing node due to timeout|Inserted node from Discovery'

# print_timeline <base epoch> <marker lines file>: both role logs, the agent node's ieee1905 log (svc1) and the markers, merged by time, in seconds
# from the base. Searches and Topology Queries come every second, so they are summarised at the end.
print_timeline() {
    local base=$1 markers=$2
    {
        timeline_source agent "$WORK/agent.snap" 0 "$AGENT_TIMELINE" || return 1
        timeline_source ctrl "$WORK/ctrl.snap" 0 "$CTRL_TIMELINE" || return 1
        timeline_source svc1 "$WORK/service1.snap" 0 "$SERVICE_TIMELINE" || return 1
        cat "$markers"
    } | sort -k2,2n | awk -v base="$base" '
        {
            time = $2 - base
            text = ""
            for (i = 5; i <= NF; i++) { text = text " " $i }
            if (text ~ /ieee1905::/) {
                sub(/^ .*ieee1905::[a-z_]+: /, "", text)
            } else {
                sub(/^ [a-z_:0-9]+: /, "", text)
            }
            if (text ~ /^Search sent|^Topology Query sent/) {
                key = $1 " " $5 " " text
                sub(/ mid=[0-9]+/, "", key)
                if (!(key in count)) { first[key] = time }
                count[key]++
                last[key] = time
                next
            }
            printf "%+8.1fs  %-7s %s\n", time, $1, text
        }
        END {
            for (key in count) {
                printf "          %s: %d lines from %+.1fs to %+.1fs\n", key, count[key], first[key], last[key]
            }
        }'
}

# yesno <description> <number of matching lines>
yesno() { if [ "$2" -gt 0 ]; then echo "  yes: $1 ($2)"; else echo "  NO:  $1"; fi; PREDICTION_HELD=$((PREDICTION_HELD + (($2 > 0) ? 0 : 1))); }

# One Renew retry episode sends RENEW_PER_EPISODE "renew sent" lines per radio: one at once, then one per
# idle second while the radio is misconfigured (em-ctrl s2.rs send_renew).
RENEW_PER_EPISODE=5
RENEW_GATE_FAILED=0

# check_renew_retries <block install epoch> <block removal epoch>
# Gating check 5r. For each radio the controller log must hold exactly RENEW_PER_EPISODE "renew sent" lines
# between the installation and the removal of the block, and the same number over the whole log after the
# installation (no extra one once the block lifts). Zero lines fail like any other count. Prints PASS or FAIL.
check_renew_retries() {
    local install=$1 remove=$2 radio all inside after failure="" evidence=""
    local renews
    if ! snapshot_both; then
        echo "FAIL 5r renew retry: could not read the role logs"
        return 1
    fi
    if ! renews=$(C "renew sent"); then
        echo "FAIL 5r renew retry: scanning the controller log failed"
        return 1
    fi
    for radio in "$RADIO_24" "$RADIO_5"; do
        all=$(for_radio "$renews" "$radio" | awk -v a="$install" '$1 > a')
        inside=$(printf '%s\n' "$all" | awk -v b="$remove" '$1 < b' | grep -c .)
        after=$(printf '%s\n' "$all" | grep -c .)
        evidence+="${evidence:+; }$radio: $inside inside the block, $after after its installation, at $(
            printf '%s\n' "$all" | awk -v base="$install" 'NF { printf "%+.1fs ", $1 - base }')"
        if [ "$inside" -ne "$RENEW_PER_EPISODE" ] || [ "$after" -ne "$RENEW_PER_EPISODE" ]; then
            failure+="${failure:+; }$radio has $inside inside the block and $after after its installation (each must be exactly $RENEW_PER_EPISODE)"
        fi
    done
    if [ -n "$failure" ]; then
        echo "FAIL 5r renew retry: $failure ($evidence)"
        return 1
    fi
    echo "PASS 5r renew retry, exactly $RENEW_PER_EPISODE renew sent per radio in the block and none after ($evidence)"
}

# renews_in_block <radio> <install epoch> <removal epoch>: the number of "renew sent" lines of that radio in the block.
renews_in_block() {
    for_radio "$(C "renew sent")" "$1" | awk -v a="$2" -v b="$3" '$1 > a && $1 < b' | grep -c .
}

eviction_run() {
    local drop_found=0 waited=0 install_epoch remove_epoch tc_output settle=0 end_epoch
    local markers=$WORK/markers.txt
    start_nodes 4 "--test-drop-first-m2"
    echo "INFO controllers registered; waiting for 'M2 dropped by test switch'"
    # Poll every 0.2 s: the block should start as soon as the M2 is dropped. A failed read is not a match.
    while [ "$waited" -lt $((ONBOARD_WAIT_S * 5)) ]; do
        if [ "$(docker exec "$NODE2" grep -cF "M2 dropped by test switch" "$LOG_DIR/$CTRL_LOG")" = 1 ]; then
            drop_found=1
            break
        fi
        sleep 0.2
        waited=$((waited + 1))
    done
    if [ "$drop_found" -ne 1 ]; then
        bail "the controller never logged 'M2 dropped by test switch'"
    fi
    install_epoch=$(now_epoch)
    if ! docker exec "$NODE2" tc qdisc add dev "$IFACE" clsact ||
        ! docker exec "$NODE2" tc filter add dev "$IFACE" egress protocol 0x893a prio 1 matchall action drop ||
        ! docker exec "$NODE2" tc filter add dev "$IFACE" egress protocol 0x88cc prio 2 matchall action drop; then
        bail "could not install the tc egress filter in $NODE2"
    fi
    echo "INFO block installed on $NODE2 $IFACE egress (0x893a, 0x88cc) at $install_epoch for ${BLOCK_S}s"
    sleep_until "$(fsum "$install_epoch" "$BLOCK_S")"
    tc_output=$(docker exec "$NODE2" tc -s filter show dev "$IFACE" egress 2>&1)
    if ! docker exec "$NODE2" tc qdisc del dev "$IFACE" clsact; then
        bail "could not remove the tc filter in $NODE2"
    fi
    remove_epoch=$(now_epoch)
    echo "INFO block removed at $remove_epoch"
    # Wait until both radios are topo_synchronized on the agent after the block, or give up.
    while [ "$settle" -lt "$EVICTION_SETTLE_S" ]; do
        if snapshot_both &&
            [ "$(A "topo_synchronized radio=" | awk -v t="$remove_epoch" '$1 > t' | grep -c .)" -ge 2 ] &&
            [ "$(C "topo_synchronized radio=" | awk -v t="$remove_epoch" '$1 > t' | grep -c .)" -ge 2 ]; then
            break
        fi
        sleep 2
        settle=$((settle + 2))
    done
    sleep 3
    if ! snapshot_both; then
        bail "could not read the role logs after the run"
    fi
    collect_d9
    # ieee1905 colours its log; the timestamp must be the first field.
    if ! docker exec "$NODE1" cat "$LOG_DIR/ieee1905.log" | sed 's/\x1b\[[0-9;]*m//g' >"$WORK/service1.snap"; then
        bail "could not read ieee1905.log of $NODE1"
    fi
    end_epoch=$(now_epoch)
    {
        printf 'harness %s - INFO harness: tc block installed on the controller egress\n' "$install_epoch"
        printf 'harness %s - INFO harness: tc block removed\n' "$remove_epoch"
        printf 'harness %s - INFO harness: end of run\n' "$end_epoch"
    } >"$markers"

    echo
    echo "=== Check 5 timeline (seconds from the block installation; agent and ctrl role logs, harness markers)"
    if ! print_timeline "$install_epoch" "$markers"; then
        echo "FAIL could not build the timeline"
    fi
    echo "=== tc filter statistics on $NODE2 just before removal"
    printf '%s\n' "$tc_output"

    echo "=== Check 5r (gating): Renew retry count per radio"
    if ! check_renew_retries "$install_epoch" "$remove_epoch"; then
        RENEW_GATE_FAILED=1
    fi

    echo "=== Predicted outcome of D10 check 5 (traced at the pin)"
    PREDICTION_HELD=0
    yesno "the agent radio restarted discovery after the lost M2 (no M2 within 10 s)" \
        "$(A "restart discovery" | grep -F "no M2 within 10 s of M1" | grep -c .)"
    yesno "the controller cancelled em_config at 40 s inside the block" \
        "$(C "em_config cancelled: topology sync" | awk -v a="$install_epoch" -v b="$remove_epoch" '$1 > a && $1 < b' | grep -c .)"
    # This point asks only that each radio got a renew inside the block. A cancel starts a retry episode of
    # RENEW_PER_EPISODE "renew sent" lines per radio; check 5r gates that exact count.
    yesno "the controller sent a renew for each radio inside the block (the exact count is check 5r)" \
        "$([ "$(renews_in_block "$RADIO_24" "$install_epoch" "$remove_epoch")" -ge 1 ] \
            && [ "$(renews_in_block "$RADIO_5" "$install_epoch" "$remove_epoch")" -ge 1 ] \
            && echo 1 || echo 0)"
    yesno "the agent's ieee1905-rs evicted the controller (Removing node due to timeout, in the block)" \
        "$(grep -F "Removing node due to timeout" "$WORK/service1.snap" | grep -cF "al_mac=$NODE2_MAC")"
    yesno "the agent's onewifi_cb timed out after the block and it restarted discovery for that reason" \
        "$([ "$(A "onewifi_cb cancelled" | awk -v t="$remove_epoch" '$1 > t' | grep -c .)" -ge 1 ] \
            && [ "$(A "restart discovery" | grep -F "onewifi_cb timeout" | awk -v t="$remove_epoch" '$1 > t' | grep -c .)" -ge 1 ] \
            && echo 1 || echo 0)"
    yesno "the agent submitted a second dev_init (needs exactly 2 in total)" \
        "$([ "$(A "dev_init submitted" | grep -c .)" -eq 2 ] && echo 1 || echo 0)"
    yesno "both radios topo_synchronized on the agent after the block" \
        "$([ "$(A "topo_synchronized radio=" | awk -v t="$remove_epoch" '$1 > t' | grep -c .)" -ge 2 ] && echo 1 || echo 0)"
    yesno "both controller radio nodes topo_synchronized after the block" \
        "$([ "$(C "topo_synchronized radio=" | awk -v t="$remove_epoch" '$1 > t' | grep -c .)" -ge 2 ] && echo 1 || echo 0)"
    if [ "$PREDICTION_HELD" -eq 0 ]; then
        echo "RESULT 5 the predicted outcome happened (evidence only, not a gating check)"
    else
        echo "RESULT 5 the predicted outcome did NOT fully happen: $PREDICTION_HELD of 8 points differ (evidence only, not a gating check)"
    fi
    print_d9
    if [ "$RENEW_GATE_FAILED" -ne 0 ]; then
        dump_logs
        echo "check 5r failed"
        exit 1
    fi
    exit 0
}


# ---- Main
if [ "$MODE" = eviction ]; then
    eviction_run
fi

# Normal run: checks 1, 2, 3 and 6.
start_nodes 1 ""
wait_both_synchronized 3
check_normal_run
service_restart_check "$NODE2" "$CTRL_LOG" "$NODE2_MAC" "ieee1905 killed in $NODE2 (em-ctrl)"
service_restart_check "$NODE1" "$AGENT_LOG" "$NODE1_MAC" "ieee1905 killed in $NODE1 (em-agent)"

# Lost-M2 run: check 4, on fresh containers.
"${COMPOSE[@]}" down --timeout 5 >/dev/null 2>&1
start_nodes 4 "--test-drop-first-m2"
wait_both_synchronized 4
check_lost_m2

finish
