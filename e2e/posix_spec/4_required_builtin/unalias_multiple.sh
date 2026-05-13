#!/bin/sh
# POSIX_REF: 4 Utilities - unalias
# DESCRIPTION: unalias accepts multiple names
# EXPECT_EXIT: 0
alias g1='echo 1' g2='echo 2'
unalias g1 g2
