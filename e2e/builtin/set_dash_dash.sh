#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: set -- replaces positional parameters
# EXPECT_OUTPUT<<END
# 3
# a
# b
# c
# END
set -- a b c
echo "$#"
echo "$1"
echo "$2"
echo "$3"
