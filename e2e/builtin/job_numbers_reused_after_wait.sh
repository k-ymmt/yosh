#!/bin/sh
# POSIX_REF: 4 Utilities/jobs
# DESCRIPTION: jobs reports a finished job as Done once and drops it; wait drops the jobs it waited for; job numbers are reused
# EXPECT_OUTPUT<<END
# first: Done
# second: none
# after jobs: [1]
# after wait: [1]
# wait again: 3
# kept: [2] [3]
# END
f=$TEST_TMPDIR/jobs.out
sleep 0 & sleep 0.3; jobs >"$f"
case $(cat "$f") in *Done*) echo "first: Done" ;; *) echo "first: none" ;; esac
jobs >"$f"
case $(cat "$f") in *Done*) echo "second: Done" ;; *) echo "second: none" ;; esac
sleep 5 &
jobs >"$f"
printf 'after jobs: %s\n' "$(cut -c1-3 "$f")"
kill %1
wait
sleep 5 &
jobs >"$f"
printf 'after wait: %s\n' "$(cut -c1-3 "$f")"
kill %1
wait %1
(exit 3) & p=$!
wait "$p"
wait "$p"
echo "wait again: $?"
sleep 5 &
sleep 5 &
kill %1
wait %1
sleep 5 &
jobs >"$f"
printf 'kept: %s\n' "$(cut -c1-3 "$f" | tr '\n' ' ' | sed 's/ $//')"
kill %2 %3
wait
