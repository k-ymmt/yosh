#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: >&- closes an output fd; subsequent >&N on the same fd fails
# EXPECT_EXIT: 1
# EXPECT_STDERR: yosh:
f="$TEST_TMPDIR/dup_out_close"
exec 3> "$f"
exec 3>&-
echo gone >&3
