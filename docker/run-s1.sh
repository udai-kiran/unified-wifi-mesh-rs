#!/bin/bash
# Builds the harness, starts two nodes in HARNESS_MODE=s1 and runs exit checks 1 to 6 of docs/0012 D8:
# em-agent (node 1, radios 2.4 GHz and 5 GHz) and em-ctrl (node 2) over the real ieee1905-rs.
# Prints "PASS <n> <name> (<evidence>)" or "FAIL <n> <name>: <reason>" per check, in order, at the end;
# exits non-zero on any failure. KEEP=1 leaves the containers running afterwards.
#
# Every search of a log starts after a line number or a timestamp that the event under test follows,
# and a check that reads zero matching lines fails: no check passes because nothing was logged.
# Check 7 of D8 (workspace checks on stable and 1.85.0) is cargo, not this script.
set -u

cd "$(dirname "$0")/.." || exit 2
COMPOSE=(docker compose -f docker/compose.yaml)
export HARNESS_MODE=s1

NODE1=em-s0-node1 # agent
NODE2=em-s0-node2 # controller
NODE1_MAC=02:42:c0:a8:65:02
NODE2_MAC=02:42:c0:a8:65:03
RADIO_24=02:00:00:00:01:01
RADIO_5=02:00:00:00:01:02
AGENT_RADIOS="--radio $RADIO_24,0 --radio $RADIO_5,1"
AGENT_LOG=em-agent.log
CTRL_LOG=em-ctrl.log
LOG_DIR=/var/log/s0

REGISTER_WAIT_S=60
READY_WAIT_S=30 # how long the entrypoint may take to write role.ready
FOUND_LIMIT_S=60 # D8 check 3: from the later registration
FOUND_WAIT_S=100 # how long to wait for a found line before giving up; the limit is judged on log times
CTRL_DELAY_S=15  # D8 check 2: em-ctrl starts at least this long after the agent's first Search
# Check 5: the longest a radio could still search. dev_init is cancelled 10 s after the last tick that
# saw a searching node, on a 250 ms tick, with whole seconds (0012 D2, up to 11.25 s), and a resend
# comes each second, so a Search from the cancel path would show by 12.25 s. 13 s covers it.
STOP_WINDOW_S=13
# Check 6a: after the new session, a resubmitted dev_init or a Search would show within one proto
# tick (1 s) plus one orchestrator tick; 8 s is several periods.
SESSION_WINDOW_S=8

declare -A CHECK_NAME=(
    [1]="both register"
    [2]="searching past 10 s"
    [3]="peer model, Responses and found controller within 60 s"
    [4]="profile 3 with KiBMiB, then profile 2 accepted"
    [5]="each radio stops resending"
    [6]="ieee1905-rs and em-agent restarts"
)
declare -A CHECK_EVIDENCE CHECK_FAILURES
failures=0

WORK=$(mktemp -d)
SCAN=""

teardown() {
    rm -rf "$WORK"
    if [ "${KEEP:-0}" = 1 ]; then
        echo "KEEP=1: containers left running (HARNESS_MODE=s1 docker compose -f docker/compose.yaml down to remove)"
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
    for n in 1 2 3 4 5 6; do
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
    finish
}

# service_restart_check <node> <role log of the node> <AL MAC of the node> <label>
# D8 check 6a for one node: kill its ieee1905-rs, then the role process logs a new session with the
# same AL MAC and nothing else happens in either role process after a baseline taken before the kill.
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
    # The role log of the killed node, from the baseline taken before the kill.
    role_skip=$ctrl_skip
    if [ "$node" = "$NODE1" ]; then
        role_skip=$agent_skip
    fi
    if ! wait_scan "$node" "$role_log" "new session, same AL MAC" "$role_skip" 1 "$REGISTER_WAIT_S"; then
        flag 6 "$label: no 'new session, same AL MAC' in $role_log of $node within ${REGISTER_WAIT_S}s of the kill"
        return
    fi
    session=$(first_of "$SCAN")
    session_epoch=$(echo "$session" | cut -d' ' -f1)
    session_line=$(echo "$session" | cut -d' ' -f2)
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

    # Both role logs are searched after the baseline taken before the kill, not after the new-session
    # line, so a line logged between the kill and the new session counts.
    expect_none 6 "$NODE1" "$AGENT_LOG" "$agent_skip" 0 "$label" \
        "Search sent" "radio found controller" "controller found controller=" "dev_init submitted" \
        "dev_init has no candidates" "em-agent starting" "Response rejected"
    expect_none 6 "$NODE2" "$CTRL_LOG" "$ctrl_skip" 0 "$label" \
        "peer model created" "Response sent" "em-ctrl starting"
    if ! agent_pid_after=$(docker exec "$NODE1" role-ctl pid) || ! ctrl_pid_after=$(docker exec "$NODE2" role-ctl pid); then
        flag 6 "$label: could not read the role pids after the kill"
    elif [ "$agent_pid_after" != "$agent_pid" ] || [ "$ctrl_pid_after" != "$ctrl_pid" ]; then
        flag 6 "$label: a role process was restarted (its pid changed)"
    fi
    note 6 "(a) $label: new session at line $session_line with AL MAC $mac, and after the pre-kill baseline (agent log line $agent_skip, controller log line $ctrl_skip) no Search sent, radio found, dev_init submitted, em-agent or em-ctrl starting, Response rejected, peer model created or Response sent; role pids unchanged; waited ${SESSION_WINDOW_S}s after the new session"
}

# restart_agent <profile>
# Restarts em-agent through the supervisor with the given profile and waits for both radios to find
# the controller. Leaves AGENT_SKIP and CTRL_SKIP (line counts before the restart), START_LINE and
# START_EPOCH (the new process's "em-agent starting" line) and FOUND (its "radio found controller"
# lines) set. Returns 1 with a reason in RESTART_ERROR.
restart_agent() {
    local profile=$1 start
    if ! AGENT_SKIP=$(total_lines "$NODE1" "$AGENT_LOG") || ! CTRL_SKIP=$(total_lines "$NODE2" "$CTRL_LOG"); then
        RESTART_ERROR="could not read the role logs for the baseline"
        return 1
    fi
    # shellcheck disable=SC2086 # AGENT_RADIOS is several words
    if ! docker exec "$NODE1" role-ctl restart $AGENT_RADIOS --profile "$profile"; then
        RESTART_ERROR="role-ctl restart failed"
        return 1
    fi
    if ! wait_scan "$NODE1" "$AGENT_LOG" "em-agent starting" "$AGENT_SKIP" 1 10; then
        RESTART_ERROR="no 'em-agent starting' after the restart"
        return 1
    fi
    start=$(first_of "$SCAN")
    START_EPOCH=$(echo "$start" | cut -d' ' -f1)
    START_LINE=$(echo "$start" | cut -d' ' -f2)
    if ! echo "$start" | grep -qF "profile=$profile radios=2"; then
        RESTART_ERROR="the new process did not start with profile=$profile radios=2: $start"
        return 1
    fi
    if ! wait_scan "$NODE1" "$AGENT_LOG" "radio found controller" "$START_LINE" 2 "$FOUND_WAIT_S"; then
        RESTART_ERROR="fewer than 2 'radio found controller' lines within ${FOUND_WAIT_S}s of the restart"
        return 1
    fi
    FOUND=$SCAN
    return 0
}

# found_check <check> <label> <profile>: after a restart, both radios found the controller, with
# the controller's AL MAC and the given profile, in the agent's parsed Responses and its radio lines.
found_check() {
    local check=$1 label=$2 profile=$3 radio line parsed
    for radio in "$RADIO_24" "$RADIO_5"; do
        line=$(printf '%s\n' "$FOUND" | grep -F "radio=$radio ")
        if [ "$(lines_in "$line")" -ne 1 ]; then
            flag "$check" "$label: radio $radio has $(lines_in "$line") 'radio found controller' lines, expected 1"
        elif [ "$(echo "$line" | field controller)" != "$NODE2_MAC" ] || [ "$(echo "$line" | field profile)" != "$profile" ]; then
            flag "$check" "$label: radio $radio line is not controller=$NODE2_MAC profile=$profile: $line"
        fi
    done
    parsed=$(scan "$NODE1" "$AGENT_LOG" "controller found controller=" "$START_LINE")
    if [ "$(lines_in "$parsed")" -ne 2 ]; then
        flag "$check" "$label: expected 2 parsed Responses ('controller found'), got $(lines_in "$parsed")"
    elif [ "$(printf '%s\n' "$parsed" | grep -cF "controller=$NODE2_MAC profile=$profile ")" -ne 2 ]; then
        flag "$check" "$label: a parsed Response is not from $NODE2_MAC with profile=$profile: $parsed"
    fi
}

echo "Building and starting the harness (HARNESS_MODE=s1)"
if ! "${COMPOSE[@]}" up --build -d --force-recreate; then
    echo "FAIL 0 build-and-start: docker compose up failed"
    exit 1
fi

# ---- Wait until each entrypoint has initialised its supervisor files (role.ready), then start the agent
# alone; em-ctrl follows at least CTRL_DELAY_S after its first Search.
for node in "$NODE1" "$NODE2"; do
    waited=0
    until docker exec "$node" test -e "$LOG_DIR/role.ready"; do
        if [ "$waited" -ge "$READY_WAIT_S" ]; then
            flag 1 "$node did not write $LOG_DIR/role.ready within ${READY_WAIT_S}s"
            bail "the supervisor in $node is not ready"
        fi
        sleep 1
        waited=$((waited + 1))
    done
done
# shellcheck disable=SC2086 # AGENT_RADIOS is several words
if ! docker exec "$NODE1" role-ctl start $AGENT_RADIOS --profile 3; then
    flag 1 "role-ctl start failed in $NODE1"
    bail "cannot start em-agent"
fi
if ! wait_scan "$NODE1" "$AGENT_LOG" "em_agent::actor: registered session=" 0 1 "$REGISTER_WAIT_S"; then
    flag 1 "em-agent did not log 'registered' within ${REGISTER_WAIT_S}s"
    bail "em-agent did not register"
fi
agent_reg=$(first_of "$SCAN")
agent_reg_epoch=$(echo "$agent_reg" | cut -d' ' -f1)
agent_reg_line=$(echo "$agent_reg" | cut -d' ' -f2)
agent_reg_mac=$(echo "$agent_reg" | field al_mac)

if ! wait_scan "$NODE1" "$AGENT_LOG" "Search sent radio=" "$agent_reg_line" 1 20; then
    flag 2 "no 'Search sent' within 20s of the agent's registration"
    bail "the agent sent no Search"
fi
first_search_epoch=$(echo "$(first_of "$SCAN")" | cut -d' ' -f1)
echo "INFO agent registered, first Search at $first_search_epoch; starting em-ctrl in ${CTRL_DELAY_S}s"
sleep_until "$(fsum "$first_search_epoch" "$((CTRL_DELAY_S + 1))")"

if ! docker exec "$NODE2" role-ctl start; then
    flag 1 "role-ctl start failed in $NODE2"
    bail "cannot start em-ctrl"
fi
if ! wait_scan "$NODE2" "$CTRL_LOG" "em_ctrl::actor: registered session=" 0 1 "$REGISTER_WAIT_S"; then
    flag 1 "em-ctrl did not log 'registered' within ${REGISTER_WAIT_S}s"
    bail "em-ctrl did not register"
fi
ctrl_reg=$(first_of "$SCAN")
ctrl_reg_epoch=$(echo "$ctrl_reg" | cut -d' ' -f1)
ctrl_reg_line=$(echo "$ctrl_reg" | cut -d' ' -f2)
ctrl_reg_mac=$(echo "$ctrl_reg" | field al_mac)
wait_scan "$NODE2" "$CTRL_LOG" "em-ctrl starting" 0 1 5
ctrl_start_epoch=$(echo "$(first_of "$SCAN")" | cut -d' ' -f1)

# ---- Check 1
if [ "$agent_reg_mac" = "$NODE1_MAC" ] && [ "$ctrl_reg_mac" = "$NODE2_MAC" ]; then
    note 1 "em-agent registered at line $agent_reg_line with AL MAC $agent_reg_mac, em-ctrl at line $ctrl_reg_line with $ctrl_reg_mac"
else
    flag 1 "registered AL MACs are agent '$agent_reg_mac' (expected $NODE1_MAC) and controller '$ctrl_reg_mac' (expected $NODE2_MAC)"
fi

# ---- Wait for both radios to find the controller (checks 2 to 4 read the same lines)
if ! wait_scan "$NODE1" "$AGENT_LOG" "radio found controller" "$agent_reg_line" 2 "$FOUND_WAIT_S"; then
    flag 3 "fewer than 2 'radio found controller' lines within ${FOUND_WAIT_S}s of em-ctrl's registration"
    bail "the agent did not find the controller on both radios"
fi
found=$SCAN
later_reg_epoch=$ctrl_reg_epoch
if fgt "$agent_reg_epoch" "$ctrl_reg_epoch"; then
    later_reg_epoch=$agent_reg_epoch
fi
last_found_epoch=0
min_found_line=999999999
last_found_line=0
for radio in "$RADIO_24" "$RADIO_5"; do
    line=$(printf '%s\n' "$found" | grep -F "radio=$radio ")
    if fgt "$(echo "$line" | cut -d' ' -f1)" "$last_found_epoch"; then
        last_found_epoch=$(echo "$line" | cut -d' ' -f1)
    fi
    if [ "$(echo "$line" | cut -d' ' -f2)" -lt "$min_found_line" ]; then
        min_found_line=$(echo "$line" | cut -d' ' -f2)
    fi
    if [ "$(echo "$line" | cut -d' ' -f2)" -gt "$last_found_line" ]; then
        last_found_line=$(echo "$line" | cut -d' ' -f2)
    fi
done

# ---- Check 2: searching past 10 s
searches=$(scan "$NODE1" "$AGENT_LOG" "Search sent radio=" "$agent_reg_line")
first_search=$(first_of "$searches")
first_search_epoch=$(echo "$first_search" | cut -d' ' -f1)
head_start=$(fdiff "$ctrl_start_epoch" "$first_search_epoch")
if fge "$head_start" "$CTRL_DELAY_S"; then
    note 2 "em-ctrl started ${head_start}s after the agent's first Search"
else
    flag 2 "em-ctrl started only ${head_start}s after the agent's first Search, need ${CTRL_DELAY_S}s"
fi
for radio in "$RADIO_24" "$RADIO_5"; do
    line=$(printf '%s\n' "$found" | grep -F "radio=$radio ")
    found_epoch=$(echo "$line" | cut -d' ' -f1)
    found_line=$(echo "$line" | cut -d' ' -f2)
    series=$(printf '%s\n' "$searches" | grep -F "radio=$radio " |
        awk -v end="$found_line" '$2 < end { n = split($0, part, "mid="); print $1, part[2] + 0 }')
    stats=$(printf '%s\n' "$series" | awk '
        NF == 2 {
            n++
            if (n == 1) { first = $1 }
            if (n > 1) {
                gap = $1 - prev
                if (min == "" || gap < min) { min = gap }
                if (gap > max) { max = gap }
                if ($2 <= prevmid) { bad++ }
            }
            prev = $1
            prevmid = $2
        }
        END { printf "%d %.2f %.2f %d %s\n", n, min, max, bad, first }')
    read -r count min_gap max_gap bad_mids first_epoch <<<"$stats"
    if [ "$count" -lt 15 ]; then
        flag 2 "radio $radio: only $count Searches before it found the controller, expected about one per second for 15 s or more"
        continue
    fi
    if ! fge "$min_gap" 0.5 || fgt "$max_gap" 1.6; then
        flag 2 "radio $radio: Search gaps range from ${min_gap}s to ${max_gap}s, expected about 1 s"
    fi
    if [ "$bad_mids" -ne 0 ]; then
        flag 2 "radio $radio: $bad_mids Search MIDs did not increase"
    fi
    searched_for=$(fdiff "$found_epoch" "$first_epoch")
    if ! fgt "$searched_for" 10; then
        flag 2 "radio $radio found the controller after only ${searched_for}s of searching"
    fi
    note 2 "radio $radio: $count Searches, gaps ${min_gap}s to ${max_gap}s, MIDs increasing, controller found after ${searched_for}s"
done
# The scan ends at the later found line: dev_init is cancelled after discovery (0012 D2), so a later
# "dev_init cancelled" is normal.
expect_none 2 "$NODE1" "$AGENT_LOG" "$agent_reg_line" "$last_found_line" "searching" \
    "dev_init cancelled" "Response rejected"

# ---- Check 3
created=$(scan "$NODE2" "$CTRL_LOG" "peer model created" "$ctrl_reg_line")
created_for_agent=$(printf '%s\n' "$created" | grep -F "agent=$NODE1_MAC ")
if [ "$(lines_in "$created_for_agent")" -ne 1 ]; then
    flag 3 "controller logged $(lines_in "$created_for_agent") 'peer model created' lines for $NODE1_MAC after its registration, expected 1"
elif ! fge "$(fsum "$later_reg_epoch" "$FOUND_LIMIT_S")" "$(echo "$created_for_agent" | cut -d' ' -f1)"; then
    flag 3 "peer model created more than ${FOUND_LIMIT_S}s after the later registration"
else
    note 3 "peer model created for $NODE1_MAC at line $(echo "$created_for_agent" | cut -d' ' -f2)"
fi
responses=$(scan "$NODE2" "$CTRL_LOG" "Response sent" "$ctrl_reg_line")
for band in BAND_2_4_GHZ BAND_5_GHZ; do
    band_lines=$(printf '%s\n' "$responses" | grep -F "band=Some($band(")
    if [ "$(lines_in "$band_lines")" -lt 1 ]; then
        flag 3 "no 'Response sent' for $band after the controller's registration"
    else
        note 3 "$(lines_in "$band_lines") Responses sent for $band"
    fi
done
for radio in "$RADIO_24" "$RADIO_5"; do
    line=$(printf '%s\n' "$found" | grep -F "radio=$radio ")
    found_epoch=$(echo "$line" | cut -d' ' -f1)
    if [ "$(echo "$line" | field controller)" != "$NODE2_MAC" ]; then
        flag 3 "radio $radio found controller '$(echo "$line" | field controller)', expected $NODE2_MAC"
    elif ! fgt "$found_epoch" "$ctrl_reg_epoch"; then
        flag 3 "radio $radio found the controller before the controller registered"
    elif ! fge "$(fsum "$later_reg_epoch" "$FOUND_LIMIT_S")" "$found_epoch"; then
        flag 3 "radio $radio found the controller $(fdiff "$found_epoch" "$later_reg_epoch")s after the later registration, limit ${FOUND_LIMIT_S}s"
    else
        note 3 "radio $radio found $NODE2_MAC $(fdiff "$found_epoch" "$later_reg_epoch")s after the later registration"
    fi
done

# ---- Check 4, first half: profile 3 with the KiBMiB bit, in the agent's parsed Responses
parsed=$(scan "$NODE1" "$AGENT_LOG" "controller found controller=" "$agent_reg_line")
if [ "$(lines_in "$parsed")" -ne 2 ]; then
    flag 4 "profile 3: expected 2 parsed Responses ('controller found'), got $(lines_in "$parsed")"
elif [ "$(printf '%s\n' "$parsed" | grep -cF "controller=$NODE2_MAC profile=3 capability=Some(ControllerCapability { kib_mib_counter: true,")" -ne 2 ]; then
    flag 4 "profile 3: a parsed Response lacks profile=3 with kib_mib_counter: true: $parsed"
else
    note 4 "both parsed Responses have profile=3 and kib_mib_counter: true"
fi
other_profiles=$(printf '%s\n' "$responses" | grep -vF "profile=Some(3)")
if [ -n "$other_profiles" ] || [ "$(lines_in "$responses")" -lt 1 ]; then
    flag 4 "profile 3: the controller sent a Response with another profile, or none: $other_profiles"
fi

# ---- Check 5: after its Response, each radio stops resending
for radio in "$RADIO_24" "$RADIO_5"; do
    line=$(printf '%s\n' "$found" | grep -F "radio=$radio ")
    sleep_until "$(fsum "$(echo "$line" | cut -d' ' -f1)" "$((STOP_WINDOW_S + 1))")"
    if ! late=$(scan "$NODE1" "$AGENT_LOG" "Search sent radio=$radio " "$(echo "$line" | cut -d' ' -f2)"); then
        flag 5 "radio $radio: could not read $AGENT_LOG, so the absence of a late Search was not verified"
    elif [ -n "$late" ]; then
        flag 5 "radio $radio sent a Search after it found the controller: $(first_of "$late" | cut -d' ' -f3-)"
    else
        note 5 "radio $radio: no Search in the ${STOP_WINDOW_S}s after its found line $(echo "$line" | cut -d' ' -f2)"
    fi
done
cancelled=$(scan "$NODE1" "$AGENT_LOG" "dev_init cancelled" "$agent_reg_line")
echo "INFO 5 dev_init cancel lines in the agent log: $(lines_in "$cancelled") (the window covers the cancel)"

# ---- Check 6a: ieee1905-rs killed on either node
# Before any kill the agent has submitted dev_init exactly once.
if ! submitted=$(scan "$NODE1" "$AGENT_LOG" "dev_init submitted" 0); then
    flag 6 "(a) could not read $AGENT_LOG to count 'dev_init submitted'"
elif [ "$(lines_in "$submitted")" -ne 1 ]; then
    flag 6 "(a) expected exactly 1 'dev_init submitted' in $AGENT_LOG before the first kill, got $(lines_in "$submitted")"
else
    note 6 "(a) exactly one 'dev_init submitted' (line $(echo "$submitted" | cut -d' ' -f2)) before the first kill"
fi
service_restart_check "$NODE2" "$CTRL_LOG" "$NODE2_MAC" "ieee1905 killed in $NODE2 (em-ctrl)"
service_restart_check "$NODE1" "$AGENT_LOG" "$NODE1_MAC" "ieee1905 killed in $NODE1 (em-agent)"

# ---- Check 4, second half: restart the agent with --profile 2
if ! restart_agent 2; then
    flag 4 "profile 2: $RESTART_ERROR"
else
    found_check 4 "profile 2" 2
    profile2_responses=$(scan "$NODE2" "$CTRL_LOG" "Response sent" "$CTRL_SKIP" | awk -v start="$START_EPOCH" '$1 > start')
    if [ "$(lines_in "$profile2_responses")" -lt 1 ]; then
        flag 4 "profile 2: the controller sent no Response after the restart"
    elif [ "$(printf '%s\n' "$profile2_responses" | grep -cvF "profile=Some(2)")" -ne 0 ]; then
        flag 4 "profile 2: a Response after the restart does not carry profile 2: $profile2_responses"
    fi
    expect_none 4 "$NODE1" "$AGENT_LOG" "$START_LINE" 0 "profile 2" "Response rejected"
    note 4 "after --profile 2: $(lines_in "$profile2_responses") Responses with profile=Some(2), both radios found the controller with profile=2, none rejected"
fi

# ---- Check 6b: restart the agent again (default profile 3)
if ! restart_agent 3; then
    flag 6 "(b) $RESTART_ERROR"
else
    found_check 6 "(b)" 3
    first_new_search=$(first_of "$(scan "$NODE1" "$AGENT_LOG" "Search sent radio=" "$START_LINE")")
    if [ "$(echo "$first_new_search" | field mid)" != 1 ]; then
        flag 6 "(b) the first Search after the restart is not MID 1: $first_new_search"
    fi
    existing=$(scan "$NODE2" "$CTRL_LOG" "existing peer model agent=$NODE1_MAC" "$CTRL_SKIP" | awk -v start="$START_EPOCH" '$1 > start')
    if [ "$(lines_in "$existing")" -lt 1 ]; then
        flag 6 "(b) the controller logged no 'existing peer model' for $NODE1_MAC after the restart"
    fi
    if ! created_again=$(scan "$NODE2" "$CTRL_LOG" "peer model created agent=$NODE1_MAC" "$CTRL_SKIP"); then
        flag 6 "(b) could not read $CTRL_LOG, so the absence of a second 'peer model created' was not verified"
    elif [ -n "$created_again" ]; then
        flag 6 "(b) the controller created the peer model again: $created_again"
    fi
    note 6 "(b) restart: first Search MID $(echo "$first_new_search" | field mid), controller logged 'existing peer model' $(lines_in "$existing") times after the restart and no second 'created', both radios found $NODE2_MAC"
fi

finish
