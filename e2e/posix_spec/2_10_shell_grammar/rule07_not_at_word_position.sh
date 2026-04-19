#!/bin/sh
# POSIX_REF: 2.10.2 Rule 7 - Assignment preceding command name
# DESCRIPTION: After command name, A=1 is a literal argument, not an assignment
# EXPECT_OUTPUT: A=1
# EXPECT_EXIT: 0
echo A=1
