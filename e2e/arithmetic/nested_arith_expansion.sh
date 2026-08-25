#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Nested $((...)) inside arithmetic expansion
# EXPECT_OUTPUT: 3
echo $(( $((1+1)) + 1 ))
