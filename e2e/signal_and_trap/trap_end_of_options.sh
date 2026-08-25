#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: Leading -- is end-of-options, enabling trap -- action condition
# EXPECT_OUTPUT<<END
# hi
# bye
# END
# EXPECT_EXIT: 0
trap -- 'echo bye' EXIT
echo hi
