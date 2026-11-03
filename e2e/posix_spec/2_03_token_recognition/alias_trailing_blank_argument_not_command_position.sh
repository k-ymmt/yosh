#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: A word after a trailing-blank alias is only eligible for substitution; spelling "case" there must not open a case context that swallows later commands
# EXPECT_OUTPUT<<END
# case x in x
# OK
# END
# EXPECT_EXIT: 0
alias e='echo '
alias ok='echo OK'
e case x in x
ok
