#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Conditional expansion forms work on positional parameters
# EXPECT_OUTPUT<<END
# one
# d
# e
# alt
# END
# EXPECT_EXIT: 0
# XFAIL: yosh treats positional parameters as unset in all conditional expansion forms (set -- one; ${1:-d} yields d)
set -- one
echo "${1:-d}"
echo "${2:-d}"
echo "${2-e}"
echo "${1:+alt}"
