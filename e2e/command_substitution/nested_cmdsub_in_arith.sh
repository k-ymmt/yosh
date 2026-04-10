#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution nested inside arithmetic expansion
# EXPECT_OUTPUT: 3
echo $(( $(echo 1) + $(echo 2) ))
