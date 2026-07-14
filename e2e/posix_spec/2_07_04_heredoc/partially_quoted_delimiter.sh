#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: A partially quoted delimiter counts as quoted and suppresses expansion
# EXPECT_OUTPUT: $v
# EXPECT_EXIT: 0
v=1
cat <<E"OF"
$v
EOF
