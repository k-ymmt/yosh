#!/bin/sh
# POSIX_REF: 2.10.2 Rule 10 - Keyword recognition
# DESCRIPTION: Reserved word is recognized in command position after a pipe
# EXPECT_OUTPUT: x
# EXPECT_EXIT: 0
# NOTE: If `if` after the pipe were NOT recognized as a reserved word,
# the parser would treat `if` as an external command name; lookup would
# fail and exit 127 (command not found) with empty output, instead of
# the if-statement running `cat` and printing `x`.
echo x | if true; then cat; fi
