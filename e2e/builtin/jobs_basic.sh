#!/bin/sh
# POSIX_REF: 2.11 Job Control
# DESCRIPTION: jobs builtin lists background jobs
# EXPECT_EXIT: 0
# EXPECT_STDERR: [1]
sleep 0.1 &
jobs
wait
