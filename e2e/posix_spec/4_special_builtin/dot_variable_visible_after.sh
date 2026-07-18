#!/bin/sh
# POSIX_REF: 2.15 dot
# DESCRIPTION: variables set inside dot script persist in the current shell
# EXPECT_OUTPUT: persisted
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/v.sh" <<'EOF'
v=persisted
EOF
. "$TEST_TMPDIR/v.sh"
echo "$v"
