#!/bin/bash
# Runs ieee1905-rs in a restart loop (so check 5 can kill it without stopping the container),
# then runs the AL-SAP probe in the foreground.
#
# Environment:
#   ROLE          agent | controller (required)
#   IFACE         interface ieee1905-rs forwards on (default eth0)
#   CONTROL_PATH  AL-SAP control socket (default: upstream's per-role path)
#   DATA_PATH     AL-SAP data socket    (default: upstream's per-role path)
#   LOG_DIR       log directory (default /var/log/s0)
#   FIFO          probe command FIFO (default /run/s0/probe.fifo)
set -u

case "${ROLE:-}" in
agent)
    default_control=/tmp/al_control_socket
    default_data=/tmp/al_data_socket
    ;;
controller)
    default_control=/tmp/al_em_ctrl_control_socket
    default_data=/tmp/al_em_ctrl_data_socket
    ;;
*)
    echo "ROLE must be agent or controller, got '${ROLE:-}'" >&2
    exit 2
    ;;
esac

IFACE=${IFACE:-eth0}
CONTROL_PATH=${CONTROL_PATH:-$default_control}
DATA_PATH=${DATA_PATH:-$default_data}
LOG_DIR=${LOG_DIR:-/var/log/s0}
FIFO=${FIFO:-/run/s0/probe.fifo}

mkdir -p "$LOG_DIR" "$(dirname "$FIFO")"
[ -p "$FIFO" ] || mkfifo "$FIFO"

# Debug level on the ieee1905 crate: the "Inserted node from Discovery" line that
# run-s0.sh waits for is logged at debug. ieee1905-rs removes stale socket files itself.
supervise_ieee1905() {
    while true; do
        ieee1905 --interface "$IFACE" \
            --sap-control-path "$CONTROL_PATH" \
            --sap-data-path "$DATA_PATH" \
            --filter ieee1905=debug >>"$LOG_DIR/ieee1905.log" 2>&1 &
        echo $! >"$LOG_DIR/ieee1905.pid"
        wait $!
        echo "ieee1905 exited with status $?, restarting" >>"$LOG_DIR/supervisor.log"
        sleep 1
    done
}
supervise_ieee1905 &

# The probe stdout is the probe log that run-s0.sh reads.
exec alsap_probe --role "$ROLE" --control "$CONTROL_PATH" --data "$DATA_PATH" \
    --fifo "$FIFO" >>"$LOG_DIR/probe.log" 2>"$LOG_DIR/probe.err"
