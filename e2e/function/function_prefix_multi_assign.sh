#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Multiple prefix assignments scoped to function call
# EXPECT_OUTPUT<<END
# 1 2
# original_a original_b
# END
A=original_a
B=original_b
show() { echo "$A $B"; }
A=1 B=2 show
echo "$A $B"
