#!/bin/sh
# POSIX_REF: 2.10.2 Rule 3 - Here-document delimiter
# DESCRIPTION: A reserved word may be used as an unquoted here-document delimiter
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
cat <<if
hello
if
