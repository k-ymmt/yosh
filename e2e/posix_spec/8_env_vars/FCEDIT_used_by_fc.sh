#!/bin/sh
# POSIX_REF: 8 Environment Variables - FCEDIT
# DESCRIPTION: FCEDIT selects the editor used by fc with no -e option
# MIGRATED_TO: tests/pty_posix.rs::fcedit::used_by_fc
# EXPECT_EXIT: 0
FCEDIT=cat
fc 2>&1 >/dev/null </dev/null
