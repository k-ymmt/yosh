#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${name} delimits the parameter from following text
# EXPECT_OUTPUT: abcd
# EXPECT_EXIT: 0
x=ab
echo "${x}cd"
