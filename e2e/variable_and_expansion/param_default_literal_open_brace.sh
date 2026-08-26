#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: A literal { inside ${x:-...} default text in a plain word needs no closer
# EXPECT_OUTPUT<<END
# {
# {}
# END
unset x
echo ${x:-{}
echo ${x:-{}}
