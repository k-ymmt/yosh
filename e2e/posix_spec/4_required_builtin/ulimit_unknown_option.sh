#!/bin/sh
# POSIX_REF: 4 Utilities - ulimit
# DESCRIPTION: ulimit with unknown option is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: ulimit
ulimit -Z
