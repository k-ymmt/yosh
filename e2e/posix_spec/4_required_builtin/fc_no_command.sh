#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc with no operands edits the previous command (requires editor; should not crash)
# MIGRATED_TO: tests/pty_posix.rs::fc::no_args_uses_editor
# EXPECT_EXIT: 0
fc 2>&1 >/dev/null </dev/null
