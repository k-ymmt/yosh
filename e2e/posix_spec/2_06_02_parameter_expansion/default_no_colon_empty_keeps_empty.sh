#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var-word} keeps empty value (no colon = unset only)
# EXPECT_OUTPUT: [empty]
# EXPECT_EXIT: 0
x=
echo "[${x-hello}empty]"
