#!/bin/sh
# POSIX_REF: 2.14.4 colon
# DESCRIPTION: colon expands its operands (assignment via ${var:=value} side effect)
# EXPECT_OUTPUT: defaulted
# EXPECT_EXIT: 0
unset x
: ${x:=defaulted}
echo "$x"
