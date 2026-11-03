#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: trap listing single-quotes the action for re-input and names conditions without SIG
# EXPECT_OUTPUT<<END
# trap -- 'echo '\''hi'\''' INT
# trap -- '' TERM
# restored: trap -- 'echo '\''hi'\''' INT
# END
# EXPECT_EXIT: 0
trap "echo 'hi'" INT
trap '' TERM
trap
saved=$(trap)
trap - INT TERM
eval "$saved"
echo "restored: $(trap -p INT)"
