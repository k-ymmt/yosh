#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Here-document attached to compound commands (done/fi/brace/subshell)
# EXPECT_OUTPUT<<END
# got hello
# got world
# if-body
# brace
# subsh
# END
while read -r x; do echo "got $x"; done <<EOF
hello
world
EOF
if true; then cat; fi <<EOF
if-body
EOF
{ cat; } <<EOF
brace
EOF
(cat) <<EOF
subsh
EOF
