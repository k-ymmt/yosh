#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: A command terminated by a signal has $? greater than 128 (SIGTERM = 143)
# EXPECT_OUTPUT: 143
# EXPECT_EXIT: 0
sleep 5 &
p=$!
kill -TERM "$p"
wait "$p"
echo $?
