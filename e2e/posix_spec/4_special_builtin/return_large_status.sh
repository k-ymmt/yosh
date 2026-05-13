#!/bin/sh
# POSIX_REF: 2.14.12 return
# DESCRIPTION: return values are taken modulo 256 by the shell when surfaced as $?
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
f() { return 257; }
f
echo $?
