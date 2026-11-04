#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: A relative cd operand resolves against the real working directory when $PWD no longer names it
# EXPECT_OUTPUT<<END
# sub
# a
# a
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
mkdir -p a/sub
cd a || exit 1
PWD=/nonexistent-yosh-dir
cd sub || exit 1
echo "${PWD##*/}"
cd .. || exit 1
echo "${PWD##*/}"
PWD=/
cd . || exit 1
echo "${PWD##*/}"
