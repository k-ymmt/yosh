#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export of an unset variable marks it; later assignment is exported
# EXPECT_OUTPUT: later
# EXPECT_EXIT: 0
unset foo
export foo
foo=later
sh -c 'echo "$foo"'
