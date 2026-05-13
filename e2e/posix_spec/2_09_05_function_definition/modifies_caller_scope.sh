#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function assignments affect the calling shell (no local scope by default)
# EXPECT_OUTPUT: inside
# EXPECT_EXIT: 0
f() { x=inside; }
f
echo "$x"
