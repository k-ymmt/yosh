#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&N for an unopened fd N is a redirection error
# EXPECT_EXIT: 1
# EXPECT_STDERR: yosh:
echo hello >&9
