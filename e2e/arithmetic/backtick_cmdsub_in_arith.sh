#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Backtick command substitution inside arithmetic expansion
# EXPECT_OUTPUT: 3
echo $(( `echo 2` + 1 ))
