#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: $10 is $1 followed by 0, ${10} is tenth parameter
# EXPECT_OUTPUT: a0
set -- a b c d e f g h i j
echo "$10"
