#!/bin/sh
# POSIX_REF: 2.7.3 Appending Redirected Output
# DESCRIPTION: Use case - accumulate log entries with append redirection in a loop
# EXPECT_OUTPUT<<END
# entry 1
# entry 2
# entry 3
# END
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
for i in 1 2 3; do
  echo "entry $i" >> app.log
done
cat app.log
