#!/bin/sh
# POSIX_REF: 4 Utilities - bg
# DESCRIPTION: bg without job-control monitor mode is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: bg
set +m
bg >/dev/null
