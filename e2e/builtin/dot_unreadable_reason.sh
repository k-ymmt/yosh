#!/bin/sh
# POSIX_REF: 2.15 Special Built-In Utilities - dot
# DESCRIPTION: The dot diagnostic names why the file could not be read instead of always "No such file or directory"
# EXPECT_OUTPUT<<END
# yosh: .: ./adir: Is a directory
# 1
# yosh: .: ./missing: No such file or directory
# 1
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
mkdir adir
command . ./adir 2>&1
echo $?
command . ./missing 2>&1
echo $?
