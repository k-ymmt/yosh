#!/bin/sh
# POSIX_REF: 2.7.6 Duplicating an Output File Descriptor
# DESCRIPTION: cmd >f 2>&1 merges stderr into stdout target
# EXPECT_OUTPUT<<END
# out
# err
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
sh -c 'echo out; echo err >&2' >f 2>&1
cat f
