#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: An EXIT trap set inside an asynchronous list runs when that subshell exits (dash agrees)
# EXPECT_OUTPUT<<END
# t1
# w 0
# t2
# w 5
# END
{ trap 'echo t1' EXIT; true; } &
wait $!; echo "w $?"
{ trap 'echo t2' EXIT; exit 5; } &
wait $!; echo "w $?"
