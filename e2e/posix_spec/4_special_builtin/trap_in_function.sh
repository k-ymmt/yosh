#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap set inside a function remains set after function returns
# EXPECT_OUTPUT: bye
# EXPECT_EXIT: 0
f() { trap 'echo bye' EXIT; }
f
