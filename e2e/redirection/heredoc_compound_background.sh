#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Here-document on a backgrounded compound command
# EXPECT_OUTPUT: bg-body
{ cat; } <<EOF &
bg-body
EOF
wait
