#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: unset of a read-only variable fails
# EXPECT_STDERR: unset
# EXPECT_EXIT: 1
readonly foo=locked
unset foo
