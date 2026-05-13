#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Quoted delimiter suppresses parameter expansion in body
# EXPECT_OUTPUT: $HOME
# EXPECT_EXIT: 0
HOME=/tmp/h
cat <<'EOF'
$HOME
EOF
