#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Braced parameter expansion with default operator inside arithmetic
# EXPECT_OUTPUT: 4
unset x
echo $(( ${x:-3} + 1 ))
