#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of function (compound_command)
# DESCRIPTION: A subshell is a valid function body (a compound_command)
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
f() ( echo ok )
f
