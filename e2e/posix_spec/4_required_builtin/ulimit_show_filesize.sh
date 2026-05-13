#!/bin/sh
# POSIX_REF: 4 Utilities - ulimit
# DESCRIPTION: ulimit -f with no value shows the current file-size limit
# EXPECT_EXIT: 0
ulimit -f >/dev/null
