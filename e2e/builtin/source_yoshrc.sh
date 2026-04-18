#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: ~/.yoshrc is sourced on interactive startup
# EXPECT_OUTPUT: from_yoshrc
# EXPECT_EXIT: 0

TMPHOME=$(mktemp -d)
cat > "$TMPHOME/.yoshrc" <<'RCEOF'
YOSHRC_LOADED=from_yoshrc
RCEOF

. "$TMPHOME/.yoshrc"
echo "$YOSHRC_LOADED"

rm -rf "$TMPHOME"
