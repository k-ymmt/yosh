#!/bin/sh
# POSIX_REF: 2.15 set -x
# DESCRIPTION: set -x traces prefix assignments on the same line as the command
# EXPECT_OUTPUT: hi
# EXPECT_STDERR: + x=1 echo hi
set -x
x=1 echo hi
