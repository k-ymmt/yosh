#!/bin/sh
# POSIX_REF: 2.10.1 Shell Grammar Lexical Conventions
# DESCRIPTION: A '#' comment runs to end of line; the next line is an independent command
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
echo a # this comment must not consume the next line
echo b
