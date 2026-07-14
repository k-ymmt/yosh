#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: A trap for a signal received during a foreground command runs right after that command completes
# EXPECT_OUTPUT<<END
# trapped
# after-sleep
# END
# EXPECT_EXIT: 0
trap 'echo trapped' USR1
( sleep 1; kill -USR1 $$ ) &
sleep 5
echo after-sleep
