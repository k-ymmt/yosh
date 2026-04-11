#!/bin/sh
# POSIX_REF: 2.11 Job Control
# DESCRIPTION: background job tracked with job number and PID on stderr
# EXPECT_EXIT: 0
# EXPECT_STDERR: [1]
sleep 0.1 &
wait
