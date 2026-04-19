#!/bin/sh
# POSIX_REF: 2.10.2 Rule 8 - NAME in function
# DESCRIPTION: A valid NAME is accepted as a function name
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
f() { echo ok; }
f
