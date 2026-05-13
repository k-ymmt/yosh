#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -l N prints the signal name for number N
# EXPECT_OUTPUT: TERM
# EXPECT_EXIT: 0
kill -l 15
