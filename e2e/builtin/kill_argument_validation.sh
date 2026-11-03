#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill accepts signal 0 and --, rejects unknown signal numbers and a missing pid
# EXPECT_OUTPUT<<END
# 0
# 2
# 2
# 1
# KILL
# 0
# END
# EXPECT_EXIT: 0
kill -s 0 $$; echo $?
kill -99 $$ 2>/dev/null; echo $?
kill -9 2>/dev/null; echo $?
kill -s TERM -- 999999999 2>/dev/null; echo $?
kill -l -- 9
kill -0 -- $$; echo $?
