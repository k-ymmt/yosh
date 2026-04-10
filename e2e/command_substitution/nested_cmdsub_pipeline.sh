#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Command substitution with nested pipeline
# EXPECT_OUTPUT: foo
echo $(echo foo | cat | cat)
