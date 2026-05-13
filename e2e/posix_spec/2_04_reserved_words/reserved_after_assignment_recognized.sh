#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: Reserved word in command position is recognized after assignment prefix
# XFAIL: not yet implemented (TODO: reserved word not recognized after assignment prefix; yosh treats it as a command name)
# EXPECT_OUTPUT: y
# EXPECT_EXIT: 0
x=1 if true; then echo y; fi
