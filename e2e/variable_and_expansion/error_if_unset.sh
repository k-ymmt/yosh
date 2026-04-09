#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Error message with :? when variable is unset
# EXPECT_EXIT: 1
# EXPECT_STDERR: custom error
unset x
: "${x:?custom error}"
