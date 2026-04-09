#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Prefix assignment is scoped to the command environment only
# EXPECT_OUTPUT:
MY_SCOPED_VAR=hello /usr/bin/true
echo "$MY_SCOPED_VAR"
