#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Unquoted delimiter allows parameter expansion in body
# EXPECT_OUTPUT: /tmp/h
# EXPECT_EXIT: 0
HOME=/tmp/h
cat <<EOF
$HOME
EOF
