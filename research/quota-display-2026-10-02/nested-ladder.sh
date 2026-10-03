# Nested ladder check: an isolated Server and the real Dashboard at each height.
B="/Users/xlyk/Code/ovrcr-workspaces/090a055996de6d6616b3bea5891d2641/target/OVRCR GUI.app/Contents/MacOS/ovrcr"
export OVRCR_CONFIG=/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-gui-ZSOVrX/nested/config.toml OVRCR_SOCKET=/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr-gui-ZSOVrX/nested/s.sock
unset OVRCR_DASHBOARD_CONFIG OVRCR_AGENT_SOCKET OVRCR_AGENT_TOKEN OVRCR_SESSION_ID
for r in "$@"; do stty rows $r; clear; "$B"; done
stty rows 30; "$B" shutdown
