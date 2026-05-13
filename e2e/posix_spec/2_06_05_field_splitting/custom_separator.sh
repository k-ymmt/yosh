#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Custom IFS character splits fields
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
IFS=:
x=a:b:c
set -- $x
echo $#
