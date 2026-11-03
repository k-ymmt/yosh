#!/bin/sh
# POSIX_REF: 4 Utilities/wait
# DESCRIPTION: a no-operand wait drops already-finished jobs even when no job is still running, so the next job is %1 again (dash and bash agree)
# EXPECT_OUTPUT<<END
# [1]
# lines: 1
# END
f=$TEST_TMPDIR/jobs.out
sleep 0 &
sleep 1
wait
sleep 5 &
jobs >"$f"
cut -c1-3 "$f"
echo "lines: $(wc -l <"$f" | tr -d ' ')"
kill %1
wait
