#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -l -r lists entries in reverse order
# XFAIL: fc requires non-empty history; non-interactive harness has no history
# EXPECT_EXIT: 0
fc -l -r >/dev/null
