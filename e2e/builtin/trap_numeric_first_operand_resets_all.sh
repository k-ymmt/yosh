#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: An unsigned-integer first operand makes every operand a condition to reset
# EXPECT_OUTPUT<<END
# trap -- 'echo q' QUIT
# done
# END
# EXPECT_EXIT: 0
trap 'echo a' INT HUP
trap 'echo q' QUIT
trap 1 2
trap
echo done
