#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: The recursion guard only lasts while an alias's own text is read; the alias works again in later commands and words
# EXPECT_OUTPUT<<END
# a
# b
# echo echo
# END
# EXPECT_EXIT: 0
alias e=echo
e a; e b
alias e2='echo '
e2 e2 e2
