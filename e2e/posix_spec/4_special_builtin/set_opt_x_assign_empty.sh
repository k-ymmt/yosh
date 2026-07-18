#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -x traces an empty-value assignment as + name= (trailing equals)
# EXPECT_STDERR: + empty_var=
# EXPECT_EXIT: 0
set -x
empty_var=
