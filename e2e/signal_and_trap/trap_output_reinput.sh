#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: trap output can be saved and re-input via eval (save/restore idiom)
# EXPECT_OUTPUT<<END
# T
# done
# END
# EXPECT_EXIT: 0
trap 'echo T' USR1
t=$(trap)
trap - USR1
eval "$t"
kill -USR1 $$
echo done
