#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -e EDITOR picks the editor for the edit step
# MIGRATED_TO: tests/pty_posix.rs::fc::editor_dash_e
# EXPECT_EXIT: 0
fc -e cat 2>&1 >/dev/null </dev/null
