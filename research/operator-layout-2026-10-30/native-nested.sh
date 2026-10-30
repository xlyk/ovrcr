#!/bin/sh
export OVRCR_HOME="/private/tmp/ovrcr-layout-native-6lbcvrzq/nested" OVRCR_SOCKET="/private/tmp/ovrcr-layout-native-6lbcvrzq/nested/server.sock"
cd "/Users/xlyk/.codex/worktrees/operator-layout-design/ovrcr/target/OVRCR GUI.app/Contents/MacOS"
unset OVRCR_DASHBOARD_CONFIG OVRCR_AGENT_SOCKET OVRCR_AGENT_TOKEN OVRCR_SESSION_ID
rtk proxy stty rows 16 cols 60
rtk proxy printf 'NESTED_KERNEL_%s\n' "$(rtk proxy stty size)"
rtk proxy ./ovrcr
rtk proxy stty rows 22 cols 80
rtk proxy ./ovrcr shutdown --kill
