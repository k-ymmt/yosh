#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -l -n suppresses leading numbers in the listing
# MIGRATED_TO: tests/pty_posix.rs::fc::list_no_numbers
# EXPECT_EXIT: 0
fc -l -n >/dev/null
