#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Heredoc piped to another command
# XFAIL: Phase 4 limitation — heredoc + pipeline produces empty output
# EXPECT_OUTPUT: HELLO
cat <<EOF | tr a-z A-Z
hello
EOF
