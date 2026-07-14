#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: Brace group and subshell exit status is that of the compound list
# EXPECT_OUTPUT<<END
# 1
# 0
# 1
# END
# EXPECT_EXIT: 0
{ false; }
echo $?
{ false; true; }
echo $?
( false )
echo $?
