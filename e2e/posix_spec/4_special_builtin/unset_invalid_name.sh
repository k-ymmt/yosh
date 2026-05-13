#!/bin/sh
# POSIX_REF: 2.14.18 unset
# DESCRIPTION: unset with invalid identifier is an error
# XFAIL: non-POSIX deviation (yosh does not reject invalid identifiers in unset)
# EXPECT_STDERR: unset
# EXPECT_EXIT: 1
unset 1foo
