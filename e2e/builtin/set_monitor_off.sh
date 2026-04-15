#!/bin/sh
# POSIX_REF: 2.11 Job Control
# DESCRIPTION: set +m disables monitor mode; fg/bg report no job control
# EXPECT_EXIT: 1
# EXPECT_STDERR: no job control
set +m
fg
