#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: LINENO advances past heredoc body lines
# EXPECT_OUTPUT: 10
# EXPECT_EXIT: 0
cat <<EOF >/dev/null
alpha
beta
EOF
echo $LINENO
