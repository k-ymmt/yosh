#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Assignment without a command name sets the variable in the current shell
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
x=value
echo "$x"
