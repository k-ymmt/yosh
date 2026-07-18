#!/bin/sh
# POSIX_REF: 2.15 unset
# DESCRIPTION: unset -v removes variable but leaves same-name function intact
# EXPECT_OUTPUT: function
# EXPECT_EXIT: 0
foo() { echo function; }
foo=var-value
unset -v foo
foo
