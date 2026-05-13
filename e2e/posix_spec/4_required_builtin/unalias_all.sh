#!/bin/sh
# POSIX_REF: 4 Utilities - unalias
# DESCRIPTION: unalias -a removes all aliases
# EXPECT_EXIT: 0
alias g1='echo 1' g2='echo 2'
unalias -a
alias g1 2>/dev/null && exit 1
exit 0
