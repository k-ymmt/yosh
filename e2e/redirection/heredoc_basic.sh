#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Basic here-document
# EXPECT_OUTPUT: hello world
cat <<EOF
hello world
EOF
