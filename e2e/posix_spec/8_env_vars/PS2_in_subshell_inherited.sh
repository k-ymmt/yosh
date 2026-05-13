#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS2
# DESCRIPTION: PS2 is inherited by subshells
# EXPECT_OUTPUT: cont
# EXPECT_EXIT: 0
PS2=cont
( echo "$PS2" )
