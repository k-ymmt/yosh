#!/bin/sh
# POSIX_REF: 2.15 dot
# DESCRIPTION: dot reads commands from file and executes in current environment
# EXPECT_OUTPUT: imported
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/lib.sh" <<'EOF'
foo=imported
EOF
. "$TEST_TMPDIR/lib.sh"
echo "$foo"
