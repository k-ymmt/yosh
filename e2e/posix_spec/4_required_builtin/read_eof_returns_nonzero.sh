#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read returns nonzero on EOF
# EXPECT_EXIT: 1
read line </dev/null
