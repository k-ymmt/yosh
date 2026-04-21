#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: more than 4 operands reports exit 2
# EXPECT_EXIT: 2
[ a b c d e ]
