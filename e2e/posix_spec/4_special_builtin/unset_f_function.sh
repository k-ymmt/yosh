#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset -f removes a function
# XFAIL: non-POSIX deviation (yosh unset -f does not remove functions)
# EXPECT_EXIT: 127
foo() { echo hello; }
unset -f foo
foo 2>/dev/null
