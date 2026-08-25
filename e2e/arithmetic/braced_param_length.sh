#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: String-length expansion ${#var} inside arithmetic
# EXPECT_OUTPUT: 6
a=hello
echo $(( ${#a}+1 ))
