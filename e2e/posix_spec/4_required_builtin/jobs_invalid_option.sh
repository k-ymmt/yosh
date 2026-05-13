#!/bin/sh
# POSIX_REF: 4 Utilities - jobs
# DESCRIPTION: jobs -x is an unknown option (error)
# EXPECT_EXIT: 1
# EXPECT_STDERR: jobs
# XFAIL: non-POSIX deviation (yosh returns 0 for unknown option)
jobs -x >/dev/null
