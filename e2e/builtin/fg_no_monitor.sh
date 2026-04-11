#!/bin/sh
# POSIX_REF: 2.11 Job Control
# DESCRIPTION: fg errors when monitor mode disabled (scripts)
# EXPECT_EXIT: 1
# EXPECT_STDERR: no job control
fg
