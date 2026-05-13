#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -l lists recent history entries
# XFAIL: harness limitation (fc requires non-empty history; non-interactive harness has no history)
# EXPECT_EXIT: 0
fc -l >/dev/null
