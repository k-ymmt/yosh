#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export with an identifier that starts with a digit is an error
# XFAIL: non-POSIX deviation (yosh does not reject invalid identifiers in export)
# EXPECT_STDERR: export
# EXPECT_EXIT: 1
export 1foo=v
