#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Nested default expansion uses inner fallback when both outer and inner are unset
# EXPECT_OUTPUT: fallback
# EXPECT_EXIT: 0
unset x
y=fallback
echo "${x:-${y}}"
