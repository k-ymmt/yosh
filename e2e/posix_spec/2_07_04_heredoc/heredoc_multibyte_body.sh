#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Unquoted heredoc bodies preserve multi-byte UTF-8 text
# EXPECT_OUTPUT: 日本語
# EXPECT_EXIT: 0
cat <<EOF
日本語
EOF
