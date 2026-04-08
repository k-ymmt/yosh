#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: shift removes first N positional parameters
# EXPECT_OUTPUT<<END
# b c
# c
# END
set -- a b c
shift
echo "$@"
shift
echo "$@"
