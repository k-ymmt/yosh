#!/bin/sh
# POSIX_REF: 8 Environment Variables - PPID
# DESCRIPTION: PPID is set to the parent process ID
# XFAIL: PPID special variable not yet implemented (TODO: set PPID to parent process ID)
# EXPECT_EXIT: 0
[ -n "$PPID" ] || exit 1
[ "$PPID" -gt 0 ] || exit 1
