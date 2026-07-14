#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:?word} expands to the value when var is set and non-null
# EXPECT_OUTPUT: val
# EXPECT_EXIT: 0
v=val
echo "${v:?should not be printed}"
