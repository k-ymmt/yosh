#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Self-referential variable value terminates with an error, not a hang
# EXPECT_EXIT: 1
# EXPECT_STDERR: recursion
x=x
echo $((x))
