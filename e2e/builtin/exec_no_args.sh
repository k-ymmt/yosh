#!/bin/sh
# POSIX_REF: 2.15 exec
# DESCRIPTION: exec without command does not replace the shell
# EXPECT_OUTPUT: still here
exec
echo still here
