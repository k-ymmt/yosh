#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: 1-operand form: nonempty string is true
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
if [ "hello" ]; then
    echo ok
fi
