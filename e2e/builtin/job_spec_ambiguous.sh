#!/bin/sh
# POSIX_REF: 3.204 Job Control Job ID
# DESCRIPTION: %string matching two jobs reports ambiguous job spec
# EXPECT_STDERR: wait: sleep: ambiguous job spec
# EXPECT_EXIT: 127
sleep 0.1 &
sleep 0.2 &
wait %sleep
status=$?
wait
exit $status
