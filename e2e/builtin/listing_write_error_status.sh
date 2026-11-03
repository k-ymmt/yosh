#!/bin/sh
# POSIX_REF: 2.15 Special Built-In Utilities
# DESCRIPTION: Listing builtins diagnose a failed stdout write and return 1 without ending the shell (dash agrees)
# EXPECT_OUTPUT<<END
# export 1
# readonly 1
# set 1
# set-o 1
# set+o 1
# trap 1
# times 1
# cd- 1
# hash 1
# command 1
# jobs 1
# still running
# END
# EXPECT_STDERR: export: write error
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
export E=1; readonly R=1; trap 'echo' INT
export -p >&-; echo "export $?"
readonly -p >&- 2>/dev/null; echo "readonly $?"
set >&- 2>/dev/null; echo "set $?"
set -o >&- 2>/dev/null; echo "set-o $?"
set +o >&- 2>/dev/null; echo "set+o $?"
trap >&- 2>/dev/null; echo "trap $?"
times >&- 2>/dev/null; echo "times $?"
mkdir d; cd d; cd ..; cd - >&- 2>/dev/null; echo "cd- $?"
hash cat; hash >&- 2>/dev/null; echo "hash $?"
command -v cat >&- 2>/dev/null; echo "command $?"
sleep 5 & jobs >&- 2>/dev/null; echo "jobs $?"; kill $! 2>/dev/null
echo still running
