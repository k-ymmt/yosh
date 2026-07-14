#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: $0 is unchanged inside a function body
# EXPECT_OUTPUT: same
# EXPECT_EXIT: 0
outer=$0
f() { [ "$0" = "$outer" ] && echo same; }
f
