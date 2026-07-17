#!/bin/sh
# POSIX_REF: 2.14.6 return
# DESCRIPTION: return with a non-numeric argument terminates a non-interactive shell with status 2
# EXPECT_OUTPUT:
# EXPECT_STDERR: numeric argument required
# EXPECT_EXIT: 2
f() { return foo; }
f
echo not-reached
