#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: Alias value ending in a blank subjects the next word to alias substitution
# EXPECT_OUTPUT: world
# EXPECT_EXIT: 0
alias e='echo '
alias hi='world'
e hi
