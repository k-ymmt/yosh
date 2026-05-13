#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:-word} substitutes word when var is empty
# EXPECT_OUTPUT: hello
# EXPECT_EXIT: 0
x=
echo "${x:-hello}"
