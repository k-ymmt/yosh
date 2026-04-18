#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd without -P preserves logical path (no symlink resolution)
# EXPECT_OUTPUT: /tmp
# EXPECT_EXIT: 0
cd /tmp
echo "$PWD"
