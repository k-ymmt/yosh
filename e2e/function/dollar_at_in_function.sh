#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: $@ in function expands to function arguments
# EXPECT_OUTPUT<<END
# a
# b
# c
# END
each() { for i in "$@"; do echo "$i"; done; }
each a b c
