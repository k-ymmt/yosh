#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: ! negates 1-, 2-, and 3-operand forms
# EXPECT_OUTPUT: empty nempty neq
# EXPECT_EXIT: 0
[ ! "" ] && printf 'empty '        # ! "" → true
[ ! -z "x" ] && printf 'nempty '   # ! -z "x" → true
[ ! "a" = "b" ] && printf 'neq'    # ! (a = b) → true
echo
