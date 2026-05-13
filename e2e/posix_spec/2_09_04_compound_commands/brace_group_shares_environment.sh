#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: { ...; } runs in the current shell environment
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
{ x=value; }
echo "$x"
