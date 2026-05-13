#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Assignment preceding a command is scoped to that command's environment
# EXPECT_OUTPUT<<END
# scoped
# 
# END
# EXPECT_EXIT: 0
x=scoped sh -c 'echo "$x"'
echo "$x"
