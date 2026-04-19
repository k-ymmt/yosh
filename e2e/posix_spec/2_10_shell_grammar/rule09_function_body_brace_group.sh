#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of function (compound_command)
# DESCRIPTION: A brace group is a valid function body
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
f() { echo ok; }
f
