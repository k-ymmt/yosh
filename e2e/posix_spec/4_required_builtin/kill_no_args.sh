#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill with no args is an error
# EXPECT_EXIT: 2
# EXPECT_STDERR: kill
kill >/dev/null
