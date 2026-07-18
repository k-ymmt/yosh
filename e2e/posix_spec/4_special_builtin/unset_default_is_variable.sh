#!/bin/sh
# POSIX_REF: 2.15 unset
# DESCRIPTION: unset with no flag removes a variable (default behavior)
# EXPECT_OUTPUT: <>
# EXPECT_EXIT: 0
foo=v
unset foo
echo "<$foo>"
