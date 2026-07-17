#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters - *
# DESCRIPTION: Use case - join a list of values with a custom separator via IFS and $*
# EXPECT_OUTPUT: red,green,blue
set -- red green blue
old_ifs=$IFS
IFS=,
joined="$*"
IFS=$old_ifs
echo "$joined"
