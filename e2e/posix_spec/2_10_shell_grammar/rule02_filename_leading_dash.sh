#!/bin/sh
# POSIX_REF: 2.10.2 Rule 2 - Redirection filename
# DESCRIPTION: A redirection filename may begin with '-' (not treated as an option)
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
(
    cd "$TEST_TMPDIR" && echo hi > -flag && cat -- -flag
)
