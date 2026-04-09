#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function overrides a regular builtin
# EXPECT_OUTPUT: custom echo
echo() { printf 'custom echo\n'; }
echo anything
unset -f echo
