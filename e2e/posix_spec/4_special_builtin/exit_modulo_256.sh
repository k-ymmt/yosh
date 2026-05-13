#!/bin/sh
# POSIX_REF: 2.14.8 exit
# DESCRIPTION: exit status is taken modulo 256
# EXPECT_EXIT: 1
exit 257
