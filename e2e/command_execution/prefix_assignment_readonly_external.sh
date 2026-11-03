#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A prefix assignment to a readonly variable is a variable assignment error, even for an external command
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: readonly
readonly r=1
r=2 /bin/sh -c 'echo $r'
echo after
