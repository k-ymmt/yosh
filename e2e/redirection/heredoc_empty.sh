#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Empty heredoc produces empty output
# EXPECT_OUTPUT:
cat <<EOF
EOF
