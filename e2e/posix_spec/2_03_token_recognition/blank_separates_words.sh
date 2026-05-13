#!/bin/sh
# POSIX_REF: 2.3 Token Recognition
# DESCRIPTION: Unquoted blank characters separate tokens
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
set -- a b
echo $#
