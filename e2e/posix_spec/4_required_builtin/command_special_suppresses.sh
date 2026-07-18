#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command suppresses assignment persistence and exit-on-error for special built-ins
# EXPECT_OUTPUT<<END
# v=[]
# alive
# END
# EXPECT_EXIT: 0
v=1 command :
echo "v=[$v]"
command . "$TEST_TMPDIR/no_such_file_yosh" 2>/dev/null
echo alive
