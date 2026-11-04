#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: A later prefix assignment sees an earlier one for an external command too
# EXPECT_OUTPUT<<END
# 2
# 1
# END
# EXPECT_EXIT: 0
x=1
x=2 y=$x sh -c 'echo $y'
echo $x
