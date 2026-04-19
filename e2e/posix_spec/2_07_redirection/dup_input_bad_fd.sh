#!/bin/sh
# POSIX_REF: 2.7.5 Duplicating an Input File Descriptor
# DESCRIPTION: <&N for an unopened fd N is a redirection error
# EXPECT_EXIT: 1
# EXPECT_STDERR: yosh:
cat <&9
