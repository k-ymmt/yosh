#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: Trap on a signal outside the shell's default handler set takes effect
# EXPECT_OUTPUT<<END
# got
# after
# END
# EXPECT_EXIT: 0
trap 'echo got' ABRT
kill -ABRT $$
echo after
