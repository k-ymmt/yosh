#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: A command substitution parsed before an alias is defined does not see it (dash), and an alias in use is not substituted inside a substitution in its own value
# EXPECT_OUTPUT<<END
# not-found
# not-found
# a: not found
# END
# EXPECT_EXIT: 0
f() { echo $(e x 2>/dev/null || echo not-found); }
alias e=echo
f
alias g=echo; x=$(g same-line 2>/dev/null || echo not-found); echo $x
alias a='echo $(a 2>/dev/null || echo a: not found)'
a
