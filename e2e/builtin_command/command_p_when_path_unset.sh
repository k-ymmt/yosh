#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -p uses default PATH even when PATH is unset
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
unset PATH
command -p printf "hello"
