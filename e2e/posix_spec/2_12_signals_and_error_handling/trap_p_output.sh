#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: trap -p prints trap settings in re-input format
# EXPECT_OUTPUT: trap -- 'echo x' SIGUSR1
# EXPECT_EXIT: 0
trap 'echo x' USR1
trap -p > "$TEST_TMPDIR/traps"
grep USR1 "$TEST_TMPDIR/traps"
