#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Command substitution embedded in an arithmetic assignment value sets $? (bash behavior)
# EXPECT_OUTPUT: 5
true
x=$(( $(exit 5) + 1 ))
echo $?
