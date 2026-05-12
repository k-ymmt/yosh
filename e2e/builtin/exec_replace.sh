#!/bin/sh
# POSIX_REF: 2.14.6 exec
# DESCRIPTION: exec replaces the shell process
# EXPECT_OUTPUT: replaced
exec /bin/echo replaced
echo "should not reach here"
