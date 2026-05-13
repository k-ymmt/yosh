#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var=word} does not assign when var is set (even if empty)
# EXPECT_OUTPUT<<END
# 
# 
# END
# EXPECT_EXIT: 0
x=
echo "${x=hello}"
echo "$x"
