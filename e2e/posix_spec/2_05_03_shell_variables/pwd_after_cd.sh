#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: PWD reflects the current working directory after cd
# EXPECT_OUTPUT: /tmp
# EXPECT_EXIT: 0
# XFAIL: PWD resolved to physical path (e.g. /private/tmp on macOS); POSIX 'cd' without -P shall preserve logical path
cd /tmp
echo "$PWD"
