#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill accepts %jobspec operands
# EXPECT_OUTPUT: kill-ok
# EXPECT_EXIT: 0
sleep 5 &
if kill %1 2>/dev/null; then echo kill-ok; else echo kill-failed; fi
kill "$!" 2>/dev/null
:
