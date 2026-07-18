#!/bin/sh
# POSIX_REF: 2.15 unset
# DESCRIPTION: unset -v removes a variable
# EXPECT_OUTPUT: <>
# EXPECT_EXIT: 0
foo=v
unset -v foo
echo "<$foo>"
