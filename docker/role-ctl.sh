#!/bin/bash
# Stops, starts and restarts the role process (em-agent or em-ctrl) that the entrypoint supervises
# in HARNESS_MODE=s1 (0012 D8). ieee1905-rs and the container keep running. Run it with docker exec:
#
#   role-ctl start [role arguments...]     start the role process; fails if one is running
#   role-ctl stop                          stop it (SIGTERM, SIGKILL after 5 s) and keep it stopped
#   role-ctl restart [role arguments...]   stop, then start with the new arguments
#   role-ctl pid                           print the pid of the running role process, or fail
#
# It returns once the supervisor has acted: the process is gone (stop) or running (start). stop waits
# for role.stopped, which the supervisor writes only after it has reaped its child and seen role.stop,
# so the role cannot start after stop has returned. start removes role.stopped.
# State files are in LOG_DIR (default /var/log/s0): role.args, role.stop (present while stopped),
# role.stopped (the supervisor's acknowledgement of role.stop), role.pid.
set -u

LOG_DIR=${LOG_DIR:-/var/log/s0}
ARGS_FILE=$LOG_DIR/role.args
STOP_FILE=$LOG_DIR/role.stop
PID_FILE=$LOG_DIR/role.pid
STOPPED_FILE=$LOG_DIR/role.stopped

role_running() {
    [ -s "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null
}

stop_role() {
    local waited=0
    # Remove an acknowledgement of an earlier stop before asking again.
    rm -f "$STOPPED_FILE"
    touch "$STOP_FILE"
    # Signal in a loop until the supervisor acknowledges: it may be between its stop check and writing
    # the pid file, and launches the role once more. It acknowledges only with no child, and does not
    # launch again while role.stop exists.
    while [ ! -e "$STOPPED_FILE" ]; do
        if [ -e "$PID_FILE" ]; then
            if [ "$waited" -lt 25 ]; then
                kill "$(cat "$PID_FILE" 2>/dev/null)" 2>/dev/null
            else
                kill -KILL "$(cat "$PID_FILE" 2>/dev/null)" 2>/dev/null
            fi
        fi
        sleep 0.2
        waited=$((waited + 1))
        if [ "$waited" -ge 75 ]; then
            echo "role-ctl: the supervisor did not acknowledge the stop" >&2
            return 1
        fi
    done
}

start_role() {
    local waited=0
    if role_running; then
        echo "role-ctl: a role process is already running, use restart" >&2
        return 1
    fi
    # The supervisor splits the line on whitespace; none of the role arguments contains any.
    printf '%s\n' "$*" >"$ARGS_FILE.tmp"
    mv "$ARGS_FILE.tmp" "$ARGS_FILE"
    rm -f "$STOPPED_FILE" "$STOP_FILE"
    until role_running; do
        sleep 0.1
        waited=$((waited + 1))
        if [ "$waited" -ge 100 ]; then
            echo "role-ctl: the supervisor did not start the role process" >&2
            return 1
        fi
    done
}

command=${1:-}
[ "$#" -gt 0 ] && shift
case "$command" in
start) start_role "$@" ;;
stop) stop_role ;;
restart) stop_role && start_role "$@" ;;
pid) role_running && cat "$PID_FILE" ;;
*)
    echo "usage: role-ctl start|restart [role arguments...] | stop | pid" >&2
    exit 2
    ;;
esac
