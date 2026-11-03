#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: Commands before a line with a lexical error run before the error is reported
# EXPECT_OUTPUT<<END
# 1
# 2
# END
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
echo 1
echo 2
'bad
