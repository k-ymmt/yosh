#!/bin/sh
# POSIX_REF: 2.1 Shell Introduction
# DESCRIPTION: Script file operand sets $0 to the script path and $1 $2 to operands
# EXPECT_OUTPUT: myscript.sh a b
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/myscript.sh" <<'SCRIPT'
echo "${0##*/} $1 $2"
SCRIPT
./target/debug/yosh "$TEST_TMPDIR/myscript.sh" a b
