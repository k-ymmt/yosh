#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS1
# DESCRIPTION: PS1 is set to a default value when shell starts (non-empty)
# MIGRATED_TO: tests/pty_posix.rs::ps1::default_value_set
# EXPECT_EXIT: 0
[ -n "${PS1+x}" ] && exit 0
exit 1
