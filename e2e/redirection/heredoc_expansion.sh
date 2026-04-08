#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Here-document with variable expansion
# EXPECT_OUTPUT: value is hello
x=hello
cat <<EOF
value is $x
EOF
