#!/bin/sh
# POSIX_REF: 2.14.4 exit
# DESCRIPTION: exit with a non-numeric argument terminates the shell with status 2
# EXPECT_OUTPUT:
# EXPECT_STDERR: numeric argument required
# EXPECT_EXIT: 2
exit foo
echo not-reached
