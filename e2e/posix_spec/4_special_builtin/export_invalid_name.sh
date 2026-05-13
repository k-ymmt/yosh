#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export with an identifier that starts with a digit is an error
# EXPECT_STDERR: export
# EXPECT_EXIT: 1
export 1foo=v
