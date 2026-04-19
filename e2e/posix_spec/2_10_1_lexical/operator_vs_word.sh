#!/bin/sh
# POSIX_REF: 2.10.1 Shell Grammar Lexical Conventions
# DESCRIPTION: The '&&' control operator is recognized without surrounding whitespace
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
echo a&&echo b
