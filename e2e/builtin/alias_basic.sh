#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: alias defines command alias, unalias removes it
# EXPECT_OUTPUT: hello
alias greet='echo hello'
greet
