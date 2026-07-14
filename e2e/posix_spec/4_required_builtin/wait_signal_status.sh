#!/bin/sh
# POSIX_REF: wait utility
# DESCRIPTION: wait on a child terminated by a signal returns 128+N
# EXPECT_OUTPUT: 143
# EXPECT_EXIT: 0
sleep 5 &
p=$!
kill -TERM "$p"
wait "$p"
echo $?
