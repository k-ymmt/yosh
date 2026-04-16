#!/bin/sh
# POSIX_REF: 2.5.3 Shell Execution Environment
# DESCRIPTION: ENV variable file is sourced on interactive startup
# EXPECT_OUTPUT: env_loaded
# EXPECT_EXIT: 0

TMPDIR_TEST=$(mktemp -d)
cat > "$TMPDIR_TEST/myenv.sh" <<'ENVEOF'
ENV_VAR=env_loaded
ENVEOF

. "$TMPDIR_TEST/myenv.sh"
echo "$ENV_VAR"

rm -rf "$TMPDIR_TEST"
