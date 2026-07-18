#!/bin/sh
# POSIX_REF: 2.15 unset
# DESCRIPTION: unset accepts multiple names
# EXPECT_OUTPUT: <><>
# EXPECT_EXIT: 0
a=1; b=2
unset a b
echo "<$a><$b>"
