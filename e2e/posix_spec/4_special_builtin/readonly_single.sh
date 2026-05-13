#!/bin/sh
# POSIX_REF: 2.14.11 readonly
# DESCRIPTION: readonly NAME marks an existing variable read-only
# EXPECT_STDERR: readonly
# EXPECT_EXIT: 1
foo=initial
readonly foo
foo=changed
