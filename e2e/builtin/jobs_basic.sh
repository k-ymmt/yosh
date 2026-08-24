#!/bin/sh
# POSIX_REF: 2.11 Job Control
# DESCRIPTION: jobs builtin lists background jobs on stdout
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
sleep 0.1 &
jobs | grep -c '\[1\]'
wait
