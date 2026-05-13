#!/bin/sh
# POSIX_REF: 4 Utilities - wait
# DESCRIPTION: wait with no args waits for all background jobs and returns 0
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
sleep 0.01 &
sleep 0.02 &
wait
echo $?
