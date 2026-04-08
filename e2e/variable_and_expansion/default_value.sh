#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Default value with :- when variable is unset
# EXPECT_OUTPUT: fallback
unset x
echo "${x:-fallback}"
