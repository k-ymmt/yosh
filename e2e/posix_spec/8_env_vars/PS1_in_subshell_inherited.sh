#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS1
# DESCRIPTION: PS1 is inherited by subshells (variable scope only; not displayed in non-interactive)
# EXPECT_OUTPUT: my-prompt
# EXPECT_EXIT: 0
PS1='my-prompt'
( echo "$PS1" )
