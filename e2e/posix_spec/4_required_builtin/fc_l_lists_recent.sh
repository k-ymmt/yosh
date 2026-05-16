#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -l lists recent history entries
# MIGRATED_TO: tests/pty_posix.rs::fc::list_recent
# EXPECT_EXIT: 0
fc -l >/dev/null
