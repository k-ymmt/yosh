#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:-$(cmd)} — command substitution in default value
# EXPECT_OUTPUT: fallback
unset x
echo "${x:-$(echo fallback)}"
