#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: A plain external command inherits trap-ignored signals as ignored and caught signals at their default action
# EXPECT_OUTPUT<<END
# survived
# 143
# done
# END
# EXPECT_EXIT: 0
trap '' INT
trap 'echo caught' TERM
/bin/sh -c 'kill -INT $$; echo survived'
/bin/sh -c 'kill -TERM $$; echo not reached'
echo $?
echo done
