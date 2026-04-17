#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Assignment value expansion sees prior $? (no cmd sub resets it to 0)
# EXPECT_OUTPUT: x=1 after=0
# EXPECT_EXIT: 0
false
x=$?
echo "x=$x after=$?"
