#!/bin/sh
# POSIX_REF: 2.15 export
# DESCRIPTION: export NAME marks an existing variable for export
# EXPECT_OUTPUT: child-sees-foo
# EXPECT_EXIT: 0
foo=child-sees-foo
export foo
sh -c 'echo "$foo"'
