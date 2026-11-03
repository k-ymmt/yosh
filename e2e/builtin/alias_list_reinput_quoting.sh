#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias listing quotes values with embedded single quotes for re-input
# EXPECT_OUTPUT<<END
# alias a='echo '\''x'\'''
# x
# END
# EXPECT_EXIT: 0
alias a="echo 'x'"
alias a
saved=$(alias a)
unalias a
eval "$saved"
eval 'a'
