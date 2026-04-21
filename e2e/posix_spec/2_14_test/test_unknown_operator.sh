#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: unknown unary operator reports exit 2
# EXPECT_EXIT: 2
[ -Z foo ]
