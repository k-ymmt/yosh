#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Backtick command substitution nests with escaped inner backticks
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
echo `echo \`echo hi\``
