#!/bin/sh
# POSIX_REF: 2.9.3.1 Asynchronous Lists
# DESCRIPTION: Use case - run two jobs in the background and wait for both
# EXPECT_OUTPUT<<END
# result-a
# result-b
# done
# END
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
(echo result-a > a.out) &
(echo result-b > b.out) &
wait
cat a.out b.out
echo done
