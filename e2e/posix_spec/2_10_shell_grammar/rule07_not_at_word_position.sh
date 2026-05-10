#!/bin/sh
# POSIX_REF: 2.10.2 Rule 7 - Assignment preceding command name
# DESCRIPTION: After command name, A=1 is a literal argument, not an assignment
# EXPECT_OUTPUT: A=1
# EXPECT_EXIT: 0
# NOTE: If the parser regressed and treated `A=1` as an assignment despite
# its non-leading position, the assignment would consume the token and `echo`
# would print an empty line — i.e., observed output would be empty rather
# than `A=1`.
echo A=1
