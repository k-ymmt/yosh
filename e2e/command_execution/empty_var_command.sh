#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Empty variable as command — only assignments and redirects execute
# EXPECT_EXIT: 0
empty=
MY_EDGE_VAR=set $empty
test "$MY_EDGE_VAR" = "set"
