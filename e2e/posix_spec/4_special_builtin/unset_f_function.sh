#!/bin/sh
# POSIX_REF: 2.15 unset
# DESCRIPTION: unset -f removes a function
# EXPECT_EXIT: 127
foo() { echo hello; }
unset -f foo
foo 2>/dev/null
