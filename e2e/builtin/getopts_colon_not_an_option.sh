#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: A colon in optstring marks an option-argument; -: is an illegal option
# EXPECT_OUTPUT: ? :
# EXPECT_EXIT: 0
getopts :a:b o -:
echo "$o $OPTARG"
