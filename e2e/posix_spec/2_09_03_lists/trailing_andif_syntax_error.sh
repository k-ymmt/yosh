#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: A trailing && with no following command is a syntax error
# EXPECT_STDERR: expected a command after
# EXPECT_EXIT: 2
true &&
