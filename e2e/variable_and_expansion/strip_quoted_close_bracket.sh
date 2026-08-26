#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion / 2.13.1 Patterns
# DESCRIPTION: A quoted ] in a strip-form pattern cannot close a bracket expression
# EXPECT_OUTPUT<<END
# src
# sr
# abc
# END
x=src
echo "${x%[c"]"}"
echo "${x%[c]}"
y=abc
echo "${y%%"*"}"
