#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A fatal expansion error in a forked child is not negated into success by !
# EXPECT_OUTPUT<<END
# s=1
# a=1
# c=1
# END
# EXPECT_EXIT: 1
unset x
readonly x
( ! echo ${x:=new} ) 2>/dev/null; echo "s=$?"
{ ! echo ${x:=new}; } 2>/dev/null & wait $!; echo "a=$?"
out=$( ! echo ${x:=new} ) 2>/dev/null; echo "c=$?"
set -e
out=$( ! echo ${x:=new} ) 2>/dev/null
echo survived
