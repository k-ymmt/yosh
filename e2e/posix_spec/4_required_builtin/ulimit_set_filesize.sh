#!/bin/sh
# POSIX_REF: 4 Utilities - ulimit
# DESCRIPTION: ulimit -f N sets a soft file-size limit
# EXPECT_EXIT: 0
ulimit -f 100
