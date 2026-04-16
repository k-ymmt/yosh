#!/bin/sh
# POSIX_REF: 2.5.3 Shell Execution Environment
# DESCRIPTION: ~/.yoshrc is sourced before $ENV
# EXPECT_OUTPUT: second
# EXPECT_EXIT: 0

TMPDIR_TEST=$(mktemp -d)

cat > "$TMPDIR_TEST/.yoshrc" <<'RCEOF'
ORDER_VAR=first
RCEOF

cat > "$TMPDIR_TEST/env.sh" <<'ENVEOF'
ORDER_VAR=second
ENVEOF

. "$TMPDIR_TEST/.yoshrc"
. "$TMPDIR_TEST/env.sh"
echo "$ORDER_VAR"

rm -rf "$TMPDIR_TEST"
