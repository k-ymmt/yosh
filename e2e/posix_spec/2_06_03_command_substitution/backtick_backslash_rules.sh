#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Inside backticks \$ \` \\ are special; other backslashes are literal
# EXPECT_OUTPUT<<END
# <\>
# x\ty
# $x
# END
# EXPECT_EXIT: 0
echo `printf '<%s>' \\\\`
echo `printf '%s' 'x\ty'`
x=5
echo `printf '%s' '\$x'`
