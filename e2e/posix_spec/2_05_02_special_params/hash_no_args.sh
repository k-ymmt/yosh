#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $# is 0 when no positional parameters are set
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
set --
echo $#
