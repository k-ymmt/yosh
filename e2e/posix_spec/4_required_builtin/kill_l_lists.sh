#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -l lists signal names
# EXPECT_EXIT: 0
kill -l | grep -q TERM || exit 1
