#!/bin/sh
# POSIX_REF: 4 Utilities - alias
# DESCRIPTION: alias =foo defines nothing (not found, status 1); alias and unalias accept --
# EXPECT_OUTPUT<<END
# 1
# alias x='1'
# 0
# END
# EXPECT_EXIT: 0
alias '=foo' 2>/dev/null; echo $?
alias -- x=1
alias
unalias -- x; echo $?
