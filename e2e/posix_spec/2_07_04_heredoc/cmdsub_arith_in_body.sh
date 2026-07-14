#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Command substitution and arithmetic expansion occur in an unquoted-delimiter body
# EXPECT_OUTPUT: sub 3
# EXPECT_EXIT: 0
cat <<EOF
$(echo sub) $((1+2))
EOF
