#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: The full platform signal set is available to kill -l, kill -s and trap
# EXPECT_OUTPUT<<END
# SEGV
# SEGV
# trap -- 'echo w' WINCH
# caught
# END
# EXPECT_EXIT: 0
kill -l 11
kill -l 139
trap 'echo w' WINCH
trap -p WINCH
trap - WINCH
trap 'echo caught' URG
kill -s URG $$
:
