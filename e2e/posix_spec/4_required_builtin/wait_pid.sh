#!/bin/sh
# POSIX_REF: 4 Utilities - wait
# DESCRIPTION: wait PID returns the exit status of the given pid
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
sleep 0.01 &
pid=$!
wait "$pid"
echo $?
