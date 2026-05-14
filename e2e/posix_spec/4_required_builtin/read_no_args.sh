#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read with no args is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: read
read
