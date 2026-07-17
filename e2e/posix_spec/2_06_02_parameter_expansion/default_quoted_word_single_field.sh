#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Quoted portion of ${x:-word} is not field-split
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
unset x
set -- ${x:-"a b"}
echo $#
