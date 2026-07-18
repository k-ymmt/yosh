#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: Use case - clean up a temporary work file via EXIT trap
# EXPECT_OUTPUT<<END
# payload
# cleaned up
# END
workfile="$TEST_TMPDIR/work.tmp"
trap 'rm -f "$workfile"; echo "cleaned up"' EXIT
echo payload > "$workfile"
cat "$workfile"
