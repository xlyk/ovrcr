#!/bin/sh
export OVRCR_HOME="/private/tmp/ovrcr-layout-native2-rsilv0bh/nested" OVRCR_SOCKET="/private/tmp/ovrcr-layout-native2-rsilv0bh/nested/server.sock"
cd "/Users/xlyk/.codex/worktrees/operator-layout-design/ovrcr/target/OVRCR GUI.app/Contents/MacOS"
unset OVRCR_DASHBOARD_CONFIG OVRCR_AGENT_SOCKET OVRCR_AGENT_TOKEN OVRCR_SESSION_ID
rtk proxy stty rows 16 cols 60
rtk proxy stty size > "/private/tmp/ovrcr-layout-native2-rsilv0bh/nested.kernel-size"
rtk proxy ./ovrcr
nested_exit=$?
rtk proxy printf '%s\n' "$nested_exit" > "/private/tmp/ovrcr-layout-native2-rsilv0bh/nested.dashboard.exit"
rtk proxy stty rows 20 cols 60
rtk proxy ./ovrcr shutdown --kill > "/private/tmp/ovrcr-layout-native2-rsilv0bh/nested.shutdown.log" 2>&1
shutdown_exit=$?
rtk proxy printf '%s\n' "$shutdown_exit" > "/private/tmp/ovrcr-layout-native2-rsilv0bh/nested.shutdown.exit"
exit "$nested_exit"
