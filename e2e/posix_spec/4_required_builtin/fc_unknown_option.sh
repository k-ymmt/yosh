#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc with unknown option is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: fc
fc -Z
