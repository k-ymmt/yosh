#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde at start of assignment value expands to HOME
# EXPECT_OUTPUT: /tmp/foo
# EXPECT_EXIT: 0
HOME=/tmp
x=~/foo
echo "$x"
