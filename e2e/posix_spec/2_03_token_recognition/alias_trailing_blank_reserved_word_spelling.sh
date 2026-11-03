#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: After a trailing-blank alias, a word spelled like a reserved word is not in reserved-word position and is alias-substituted
# EXPECT_OUTPUT: YES
# EXPECT_EXIT: 0
alias e='echo '
alias if=YES
e if
