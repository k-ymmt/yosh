#!/bin/sh
# POSIX_REF: 8 Environment Variables - FCEDIT
# DESCRIPTION: when FCEDIT is unset, fc uses ed by default
# MIGRATED_TO: tests/pty_posix.rs::fcedit::default_ed
# EXPECT_EXIT: 0
unset FCEDIT
fc 2>&1 >/dev/null </dev/null
