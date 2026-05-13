#!/bin/sh
# POSIX_REF: 8 Environment Variables - PPID
# DESCRIPTION: subshell preserves the original PPID (per POSIX, PPID does not change in subshell)
# EXPECT_EXIT: 0
parent="$PPID"
sub=$( echo "$PPID" )
[ "$parent" = "$sub" ] || exit 1
