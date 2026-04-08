#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: set -- replaces positional parameters
# EXPECT_OUTPUT: x y z
set -- x y z
echo "$1 $2 $3"
