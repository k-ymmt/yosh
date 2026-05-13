#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:-word} substitutes var when var is set and non-empty
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
x=value
echo "${x:-hello}"
