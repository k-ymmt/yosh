#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: ENV value undergoes parameter expansion
# EXPECT_OUTPUT: expanded_ok
# EXPECT_EXIT: 0

TMPDIR_TEST=$(mktemp -d)
cat > "$TMPDIR_TEST/.shinit" <<'ENVEOF'
EXPANDED_VAR=expanded_ok
ENVEOF

HOME="$TMPDIR_TEST" ENV='$HOME/.shinit'
. "$TMPDIR_TEST/.shinit"
echo "$EXPANDED_VAR"

rm -rf "$TMPDIR_TEST"
