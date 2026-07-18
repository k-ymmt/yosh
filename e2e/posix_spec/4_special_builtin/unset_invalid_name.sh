#!/bin/sh
# POSIX_REF: 2.15 unset
# DESCRIPTION: unset with invalid identifier is an error
# EXPECT_STDERR: unset
# EXPECT_EXIT: 1
unset 1foo
