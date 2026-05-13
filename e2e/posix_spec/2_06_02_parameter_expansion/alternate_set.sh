#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:+word} substitutes word when var is set and non-empty
# EXPECT_OUTPUT: alt
# EXPECT_EXIT: 0
x=value
echo "${x:+alt}"
