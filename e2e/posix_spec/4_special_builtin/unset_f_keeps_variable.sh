#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset -f removes function but leaves same-name variable intact
# EXPECT_OUTPUT: var-value
# EXPECT_EXIT: 0
foo() { echo function; }
foo=var-value
unset -f foo
echo "$foo"
