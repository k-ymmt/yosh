#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: << reads input until matching delimiter on its own line
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
cat <<EOF
hi
EOF
