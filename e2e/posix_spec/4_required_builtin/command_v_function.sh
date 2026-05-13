#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -v on a function prints the function name
# EXPECT_OUTPUT: myfn
# EXPECT_EXIT: 0
myfn() { :; }
command -v myfn
