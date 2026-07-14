#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: Redirection failure fails the command; the shell continues
# EXPECT_OUTPUT<<END
# failed-nonzero
# alive
# END
# EXPECT_EXIT: 0
echo hi > "$TEST_TMPDIR/no_such_dir/f" 2>/dev/null
[ $? -ne 0 ] && echo failed-nonzero
echo alive
