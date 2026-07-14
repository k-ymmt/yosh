#!/bin/sh
# POSIX_REF: kill utility
# DESCRIPTION: kill accepts %jobspec operands
# EXPECT_OUTPUT: kill-ok
# EXPECT_EXIT: 0
# XFAIL: yosh kill does not recognize % jobspecs
sleep 5 &
if kill %1 2>/dev/null; then echo kill-ok; else echo kill-failed; fi
kill "$!" 2>/dev/null
:
