#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Functions modify variables in the calling environment
# EXPECT_OUTPUT: after
x=before
setx() { x=after; }
setx
echo "$x"
