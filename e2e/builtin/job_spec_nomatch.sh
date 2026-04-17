#!/bin/sh
# POSIX_REF: 3.204 Job Control Job ID
# DESCRIPTION: %string with no matching job reports no such job
# EXPECT_STDERR: no such job
# EXPECT_EXIT: 127
sleep 0.1 &
wait %bogus
status=$?
wait
exit $status
