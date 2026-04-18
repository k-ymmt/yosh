#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: Operator characters inside quotes do not start a new token
# EXPECT_OUTPUT: a|b
# EXPECT_EXIT: 0
echo 'a|b'
