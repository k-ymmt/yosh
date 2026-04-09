#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var?msg} — custom error message to stderr
# EXPECT_EXIT: 1
# EXPECT_STDERR: my custom error
unset x
: "${x?my custom error}"
