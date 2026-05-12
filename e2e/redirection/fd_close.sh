#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: Per-command "2>&-" after ">&2" closes fd 2; dup'd target on fd 1 survives
# EXPECT_STDERR: to stderr
# EXPECT_EXIT: 0
echo "to stderr" >&2 2>&-
