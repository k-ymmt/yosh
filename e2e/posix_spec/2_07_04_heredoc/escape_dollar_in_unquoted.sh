#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Backslash escapes $ in unquoted-delimiter heredoc body
# EXPECT_OUTPUT: $x
# EXPECT_EXIT: 0
x=value
cat <<EOF
\$x
EOF
