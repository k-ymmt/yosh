#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Unset variable expands to empty string
# EXPECT_OUTPUT:
unset x
echo "$x"
