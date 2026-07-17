#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Quoted portion of ${x:=word} substitutes as one field and assigns the quote-removed value
# EXPECT_OUTPUT: 1 a b
# EXPECT_EXIT: 0
unset v
set -- ${v:="a b"}
echo "$# $v"
