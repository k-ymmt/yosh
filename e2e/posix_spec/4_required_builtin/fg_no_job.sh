#!/bin/sh
# POSIX_REF: 4 Utilities - fg
# DESCRIPTION: fg with no current job is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: fg
set -m 2>/dev/null
fg %1 >/dev/null
