#!/bin/sh
# POSIX_REF: 2.15 eval
# DESCRIPTION: eval surfaces the executed command's exit status
# EXPECT_OUTPUT: 7
# EXPECT_EXIT: 0
eval "(exit 7)"
echo $?
