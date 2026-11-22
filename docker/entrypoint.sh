#!/bin/bash
# Runs ieee1905-rs in a restart loop (so the checks can kill it without stopping the container), then
#   HARNESS_MODE=s0 (default): runs the AL-SAP probe in the foreground (docs/0011 D4);
#   HARNESS_MODE=s1: runs em-agent or em-ctrl under a second supervisor that docker/role-ctl.sh
#   (installed as role-ctl) stops, starts and restarts with new arguments (docs/0012 D8).
#
# Environment:
#   ROLE          agent | controller (required)
#   HARNESS_MODE  s0 | s1 (default s0)
#   IFACE         interface ieee1905-rs forwards on (default eth0)
#   CONTROL_PATH  AL-SAP control socket (default: upstream's per-role path)
#   DATA_PATH     AL-SAP data socket    (default: upstream's per-role path)
#   LOG_DIR       log directory (default /var/log/s0)
#   FIFO          probe command FIFO (default /run/s0/probe.fifo; s0 only)
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

if [ "${HARNESS_MODE:-s0}" = s1 ]; then
    case "$ROLE" in
    agent) role_binary=em-agent ;;
    controller) role_binary=em-ctrl ;;
    esac

    # The role process starts only when role-ctl asks for it, with the arguments it gives. Until then
    # role.stop exists. The log is appended across restarts; the process logs "starting" each time.
    # role.ready tells run-s1.sh that role-ctl can be used: it is written after role.stop is created.
    rm -f "$LOG_DIR/role.pid" "$LOG_DIR/role.args" "$LOG_DIR/role.ready" "$LOG_DIR/role.stopped"
    touch "$LOG_DIR/role.stop"
    supervise_role() {
        local -a args
        while true; do
            if [ -e "$LOG_DIR/role.stop" ]; then
                # No child is running here (it was reaped below) and the next pass checks role.stop
                # again before any launch, so role-ctl stop may return once this file exists.
                touch "$LOG_DIR/role.stopped"
                sleep 0.2
                continue
            fi
            read -r -a args <"$LOG_DIR/role.args"
            RUST_LOG=info "$role_binary" "${args[@]}" >>"$LOG_DIR/$role_binary.log" 2>&1 &
            echo $! >"$LOG_DIR/role.pid"
            wait $!
            echo "$role_binary exited with status $?" >>"$LOG_DIR/supervisor.log"
            rm -f "$LOG_DIR/role.pid"
            # An exit nobody asked for is restarted after a pause, like ieee1905.
            [ -e "$LOG_DIR/role.stop" ] || sleep 1
        done
    }
    supervise_role &
    touch "$LOG_DIR/role.ready"
    wait
fi

# The probe stdout is the probe log that run-s0.sh reads.
exec alsap_probe --role "$ROLE" --control "$CONTROL_PATH" --data "$DATA_PATH" \
    --fifo "$FIFO" >>"$LOG_DIR/probe.log" 2>"$LOG_DIR/probe.err"
