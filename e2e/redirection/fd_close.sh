#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: File descriptor close with N>&-
# EXPECT_EXIT: 0
echo "to stderr" >&2 2>&-
