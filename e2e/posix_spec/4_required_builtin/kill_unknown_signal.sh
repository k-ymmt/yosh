#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -BOGUS is an error
# EXPECT_EXIT: 2
# EXPECT_STDERR: kill
kill -BOGUS 1 >/dev/null
