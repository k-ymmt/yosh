#!/bin/sh
# POSIX_REF: 2.15 dot
# DESCRIPTION: dot's exit status is the status of the last command in the file
# EXPECT_OUTPUT: 5
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/s.sh" <<'EOF'
(exit 5)
EOF
. "$TEST_TMPDIR/s.sh"
echo $?
