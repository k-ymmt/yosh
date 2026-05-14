#!/bin/sh
# POSIX_REF: 4 Utilities - jobs
# DESCRIPTION: jobs with unknown job spec is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: jobs
jobs %99 >/dev/null
