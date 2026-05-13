#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias can define multiple aliases in one call
# EXPECT_OUTPUT<<END
# hi
# bye
# END
# EXPECT_EXIT: 0
alias g1='echo hi' g2='echo bye'
g1
g2
