#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $0 holds shell or script name
# EXPECT_EXIT: 0
test -n "$0"
