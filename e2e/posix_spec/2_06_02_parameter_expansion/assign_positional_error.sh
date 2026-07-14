#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${1=word} assignment to a positional parameter is an error
# EXPECT_OUTPUT: error-ok
# EXPECT_EXIT: 0
out=$(./target/debug/yosh -c 'echo ${1=x}' 2>/dev/null)
[ $? -ne 0 ] && echo error-ok
