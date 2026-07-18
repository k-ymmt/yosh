#!/bin/sh
# POSIX_REF: 2.15 unset
# DESCRIPTION: unset of a read-only variable fails
# EXPECT_STDERR: unset
# EXPECT_EXIT: 1
readonly foo=v
unset foo
