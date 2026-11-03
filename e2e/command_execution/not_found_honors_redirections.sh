#!/bin/sh
# POSIX_REF: 2.9.1.1 Command Search and Execution
# DESCRIPTION: "command not found" is written with the command's redirections in effect
# EXPECT_OUTPUT<<END
# err created
# captured
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
nonexist_q_cmd 2>/dev/null
nonexist_q_cmd 2>err
[ -s err ] && echo err created
x=$(nonexist_q_cmd 2>&1)
case $x in *nonexist_q_cmd*) echo captured ;; esac
