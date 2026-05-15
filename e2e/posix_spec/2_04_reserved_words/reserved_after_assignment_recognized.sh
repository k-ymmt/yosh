#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: Reserved word in command position is recognized after assignment prefix
# EXPECT_OUTPUT: y
# EXPECT_EXIT: 0
x=1 if true; then echo y; fi
