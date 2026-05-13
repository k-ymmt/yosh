#!/bin/sh
# POSIX_REF: 4 Utilities - wait
# DESCRIPTION: wait surfaces a nonzero child exit status
# EXPECT_OUTPUT: 7
# EXPECT_EXIT: 0
sh -c 'exit 7' &
pid=$!
wait "$pid"
echo $?
