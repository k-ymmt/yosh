#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Quoted expansion prevents field splitting
# EXPECT_OUTPUT: 1
x="a b c"
set -- "$x"
echo "$#"
