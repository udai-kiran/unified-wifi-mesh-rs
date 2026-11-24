#!/bin/bash
# Builds the S0 harness, starts two nodes and runs exit checks 1 to 5 of docs/0011.
# Prints "PASS <n> <name>" or "FAIL <n> <name>: <reason>" per check; exits non-zero on any failure.
# KEEP=1 leaves the containers running afterwards.
set -u

cd "$(dirname "$0")/.." || exit 2
COMPOSE=(docker compose -f docker/compose.yaml)

NODE1=em-s0-node1 # agent
NODE2=em-s0-node2 # controller
NODE1_MAC=02:42:c0:a8:65:02
NODE2_MAC=02:42:c0:a8:65:03
LOG_DIR=/var/log/s0
FIFO=/run/s0/probe.fifo
LEARN_DISCOVERY_LINE="Inserted node from Discovery"
LEARN_WAIT_S=35 # more than one 30 s Topology Discovery interval
REGISTER_WAIT_S=60
EVENT_WAIT_S=10

failures=0

pass() { echo "PASS $1 $2"; }
fail() {
    echo "FAIL $1 $2: $3"
    failures=$((failures + 1))
}

teardown() {
    if [ "${KEEP:-0}" = 1 ]; then
        echo "KEEP=1: containers left running (docker compose -f docker/compose.yaml down to remove)"
    else
        "${COMPOSE[@]}" down --timeout 5 >/dev/null 2>&1
    fi
}
trap teardown EXIT

# read_log <node> <file>: print a log file from a node.
read_log() { docker exec "$1" cat "$LOG_DIR/$2" 2>/dev/null; }

# count_lines <node> <file> <fixed string>: number of matching lines.
count_lines() {
    local count
    count=$(read_log "$1" "$2" | grep -cF -- "$3")
    echo "${count:-0}"
}

# wait_for_line <node> <file> <fixed string> <lines to skip> <timeout s>
# Succeeds when a matching line appears after the first <lines to skip> lines of the file.
wait_for_line() {
    local node=$1 file=$2 needle=$3 skip=$4 timeout=$5 waited=0
    while [ "$waited" -lt "$timeout" ]; do
        if read_log "$node" "$file" | tail -n +$((skip + 1)) | grep -qF -- "$needle"; then
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done
    return 1
}

# wait_for_line_number <node> <file> <fixed string> <lines to skip> <timeout s>
# Like wait_for_line, and prints the number in the whole file of the first matching line
# after the skipped ones, so a later search can start right after it.
wait_for_line_number() {
    local node=$1 file=$2 needle=$3 skip=$4 timeout=$5 waited=0 found
    while [ "$waited" -lt "$timeout" ]; do
        found=$(read_log "$node" "$file" | tail -n +$((skip + 1)) | grep -nF -- "$needle" | head -n 1 | cut -d: -f1)
        if [ -n "$found" ]; then
            echo $((skip + found))
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done
    return 1
}

total_lines() { read_log "$1" "$2" | wc -l; }

# send_command <node> <command>: writes one line to the node's probe FIFO.
# The open blocks until the probe has the FIFO open, hence the timeout.
send_command() {
    timeout 10 docker exec "$1" bash -c 'echo "$0" >"$1"' "$2" "$FIFO"
}

# wait_learned <node> <discovery lines already seen> <deadline epoch>
# Returns 0 once the node's service logged a peer's Discovery, 1 if the deadline passed
# first. Prints how it ended. The log is read once more at the deadline, so a late
# caller still gets a real answer.
wait_learned() {
    local node=$1 seen=$2 deadline=$3
    while true; do
        if [ "$(count_lines "$node" ieee1905.log "$LEARN_DISCOVERY_LINE")" -gt "$seen" ]; then
            echo "discovery logged"
            return 0
        fi
        if [ "$(date +%s)" -ge "$deadline" ]; then
            echo "no discovery log line before the deadline"
            return 1
        fi
        sleep 1
    done
}

# wait_for_sent_mid <node> <SENT line prefix> <lines to skip> <timeout s>
# Prints the message id of the first matching SENT line of the node's probe after the
# skipped lines. The probe numbers its messages, so the id names one command.
wait_for_sent_mid() {
    local node=$1 prefix=$2 skip=$3 timeout=$4 number
    number=$(wait_for_line_number "$node" probe.log "$prefix" "$skip" "$timeout") || return 1
    read_log "$node" probe.log | sed -n "${number}p" | sed 's/.* mid=//'
}

# received_line <receiver mac> <sender mac> <type, 4 hex digits> <message id>
# The RECEIVED line of that exact message. The cmdu field starts with the CMDU header:
# version and reserved octets (0000), the type, then the message id.
received_line() {
    printf 'RECEIVED local=%s remote=%s type=0x%s cmdu=0000%s%04x' "$1" "$2" "$3" "$3" "$4"
}

# send_and_expect <check n> <name> <sender node> <command> <SENT prefix> <receiver node>
#                 <receiver mac> <sender mac> <type, 4 hex digits>
# Sends one command and passes when the receiver's probe prints that same message
# (matched by message id, so an older message with the same addresses does not count).
send_and_expect() {
    local n=$1 name=$2 sender=$3 command=$4 sent_prefix=$5 receiver=$6
    local receiver_mac=$7 sender_mac=$8 type=$9
    local receiver_skip sender_skip mid expected
    receiver_skip=$(total_lines "$receiver" probe.log)
    sender_skip=$(total_lines "$sender" probe.log)
    if ! send_command "$sender" "$command"; then
        fail "$n" "$name" "could not write to the FIFO of $sender"
        return
    fi
    if ! mid=$(wait_for_sent_mid "$sender" "$sent_prefix" "$sender_skip" "$EVENT_WAIT_S"); then
        fail "$n" "$name" "no '$sent_prefix' in the probe log of $sender within ${EVENT_WAIT_S}s"
        return
    fi
    expected=$(received_line "$receiver_mac" "$sender_mac" "$type" "$mid")
    if wait_for_line "$receiver" probe.log "$expected" "$receiver_skip" "$EVENT_WAIT_S"; then
        pass "$n" "$name"
    else
        fail "$n" "$name" "no '$expected' in the probe log of $receiver within ${EVENT_WAIT_S}s"
    fi
}

# check_unicast <check n> <name> <sender node> <sender mac> <receiver node> <receiver mac>
check_unicast() {
    local n=$1 name=$2 sender=$3 sender_mac=$4 receiver=$5 receiver_mac=$6
    send_and_expect "$n" "$name" "$sender" "send $receiver_mac ap-cap-query" \
        "SENT $receiver_mac type=0x8001 mid=" "$receiver" "$receiver_mac" "$sender_mac" 8001
}

check_both_directions() {
    local n=$1 label=$2
    check_unicast "$n" "$label node1->node2 ap-cap-query" "$NODE1" "$NODE1_MAC" "$NODE2" "$NODE2_MAC"
    check_unicast "$n" "$label node2->node1 ap-cap-query" "$NODE2" "$NODE2_MAC" "$NODE1" "$NODE1_MAC"
}

dump_logs() {
    local node file
    for node in "$NODE1" "$NODE2"; do
        for file in probe.log probe.err ieee1905.log supervisor.log; do
            echo "--- $node $file (last 30 lines)"
            read_log "$node" "$file" | tail -n 30
        done
    done
}

echo "Building and starting the harness"
if ! "${COMPOSE[@]}" up --build -d --force-recreate; then
    echo "FAIL 0 build-and-start: docker compose up failed"
    exit 1
fi

# Check 1: both nodes register and report their AL MACs.
check1_ok=1
for pair in "$NODE1:$NODE1_MAC" "$NODE2:$NODE2_MAC"; do
    node=${pair%%:*}
    mac=${pair#*:}
    if ! wait_for_line "$node" probe.log "REGISTERED $mac" 0 "$REGISTER_WAIT_S"; then
        fail 1 registered "$node did not print 'REGISTERED $mac' within ${REGISTER_WAIT_S}s"
        check1_ok=0
    fi
done
[ "$check1_ok" = 1 ] && pass 1 registered

# Check 2: learning. Each service must have learned the other node before unicast is possible.
# The deadline starts now, after both REGISTERED lines were seen (check 1), which is after the
# later service start.
deadline=$(($(date +%s) + LEARN_WAIT_S))
learned_ok=1
how1=$(wait_learned "$NODE1" 0 "$deadline") || learned_ok=0
how2=$(wait_learned "$NODE2" 0 "$deadline") || learned_ok=0
if [ "$learned_ok" = 1 ]; then
    pass 2 "learning (node1: $how1; node2: $how2)"
else
    fail 2 learning "node1: $how1; node2: $how2 (waited ${LEARN_WAIT_S}s after both probes registered)"
fi

# Check 3: unicast both ways.
check_both_directions 3 "unicast"

# Check 4: multicast AP-Autoconfiguration Search from node1 reaches node2.
send_and_expect 4 multicast-search "$NODE1" "send multicast search" \
    "SENT 01:80:c2:00:00:13 type=0x0007 mid=" "$NODE2" "$NODE2_MAC" "$NODE1_MAC" 0007

# Check 5: kill ieee1905-rs in node 2; the probe prints DISCONNECTED, then REGISTERED again.
probe_lines_before=$(total_lines "$NODE2" probe.log)
discoveries_before=$(count_lines "$NODE2" ieee1905.log "$LEARN_DISCOVERY_LINE")
if ! docker exec "$NODE2" bash -c "kill \$(cat $LOG_DIR/ieee1905.pid)"; then
    fail 5 restart "could not kill ieee1905 in $NODE2"
elif ! disconnected_line=$(wait_for_line_number "$NODE2" probe.log "DISCONNECTED" "$probe_lines_before" "$EVENT_WAIT_S"); then
    fail 5 restart "no DISCONNECTED in the probe log of $NODE2 within ${EVENT_WAIT_S}s"
else
    # Search after the DISCONNECTED line itself: the probe may already have printed
    # REGISTERED by the time this script saw DISCONNECTED.
    if ! wait_for_line "$NODE2" probe.log "REGISTERED $NODE2_MAC" "$disconnected_line" "$REGISTER_WAIT_S"; then
        fail 5 restart "no REGISTERED after DISCONNECTED in $NODE2 within ${REGISTER_WAIT_S}s"
    else
        pass 5 "restart (DISCONNECTED then REGISTERED)"
        restarted_at=$(date +%s)
        how=$(wait_learned "$NODE2" "$discoveries_before" $((restarted_at + LEARN_WAIT_S)))
        echo "INFO 5 relearning after restart: $how"
        check_both_directions 5 "unicast after restart"
    fi
fi

if [ "$failures" -ne 0 ]; then
    dump_logs
    echo "$failures check(s) failed"
    exit 1
fi
echo "All checks passed"
