#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Multiple heredocs in sequence
# EXPECT_OUTPUT<<END
# first
# second
# END
cat <<EOF1
first
EOF1
cat <<EOF2
second
EOF2
