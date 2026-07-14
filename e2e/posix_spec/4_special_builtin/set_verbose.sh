#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -v echoes input lines to stderr as they are read
# EXPECT_OUTPUT: verbose-ok
# EXPECT_EXIT: 0
# XFAIL: yosh accepts set -v but never echoes input lines
cat > "$TEST_TMPDIR/v.sh" <<'SCRIPT'
set -v
echo hi
SCRIPT
./target/debug/yosh "$TEST_TMPDIR/v.sh" >/dev/null 2>"$TEST_TMPDIR/err"
grep -q 'echo hi' "$TEST_TMPDIR/err" && echo verbose-ok
