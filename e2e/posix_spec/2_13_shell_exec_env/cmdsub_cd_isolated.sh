#!/bin/sh
# POSIX_REF: 2.13 Shell Execution Environment
# DESCRIPTION: cd in a command substitution does not affect the parent's working directory
# EXPECT_OUTPUT<<END
# pwd-var-unchanged
# cwd-unchanged
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
before=$PWD
junk=$(cd /)
[ "$PWD" = "$before" ] && echo pwd-var-unchanged
[ "$(pwd)" = "$before" ] && echo cwd-unchanged
