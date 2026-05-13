#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar - Background Operator
# DESCRIPTION: & terminates a command and runs it in the background
# EXPECT_OUTPUT: done
# EXPECT_EXIT: 0
sleep 0 &
echo done
wait
