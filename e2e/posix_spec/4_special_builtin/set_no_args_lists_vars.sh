#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set with no operands writes the current set of shell variables to stdout
# EXPECT_EXIT: 0
mymarker=mvalue
set | grep -q '^mymarker=' || exit 1
