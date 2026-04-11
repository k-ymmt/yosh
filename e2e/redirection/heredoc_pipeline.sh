#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Heredoc piped to another command
# EXPECT_OUTPUT: HELLO
cat <<EOF | tr a-z A-Z
hello
EOF
