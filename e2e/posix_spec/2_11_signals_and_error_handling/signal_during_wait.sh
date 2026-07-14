#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: A trapped signal makes wait return >128 immediately, then the trap runs
# EXPECT_OUTPUT<<END
# trap-ran
# wait-gt-128
# END
# EXPECT_EXIT: 0
trap 'echo trap-ran' USR1
( sleep 0.5; kill -USR1 $$ ) &
wait "$!"
s=$?
[ "$s" -gt 128 ] && echo wait-gt-128
