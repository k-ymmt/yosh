#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: $( (cmd) ) with a space disambiguates a subshell from arithmetic expansion
# EXPECT_OUTPUT: x
# EXPECT_EXIT: 0
echo $( (echo x) )
