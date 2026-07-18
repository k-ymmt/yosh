#!/bin/sh
# POSIX_REF: 2.15 dot
# DESCRIPTION: dot can introduce function definitions into the current shell
# EXPECT_OUTPUT: callable
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/fn.sh" <<'EOF'
mytool() { echo callable; }
EOF
. "$TEST_TMPDIR/fn.sh"
mytool
