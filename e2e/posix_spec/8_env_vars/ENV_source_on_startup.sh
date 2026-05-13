#!/bin/sh
# POSIX_REF: 8 Environment Variables - ENV
# DESCRIPTION: ENV file is sourced on interactive shell startup; non-interactive: not sourced
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/envrc" <<'EOF'
echo from-env
EOF
ENV="$TEST_TMPDIR/envrc" sh -c 'true'
