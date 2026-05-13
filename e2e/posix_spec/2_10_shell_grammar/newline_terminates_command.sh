#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar - Newline as Terminator
# DESCRIPTION: Newline terminates a command equivalent to semicolon
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
echo a
echo b
