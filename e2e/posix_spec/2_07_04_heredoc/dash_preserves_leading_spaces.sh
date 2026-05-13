#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: <<- strips only tabs, not spaces
# EXPECT_OUTPUT:   hey
# EXPECT_EXIT: 0
cat <<-EOF
  hey
EOF
