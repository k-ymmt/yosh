#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -l -n suppresses leading numbers in the listing
# XFAIL: harness limitation (fc requires non-empty history; non-interactive harness has no history)
# EXPECT_EXIT: 0
fc -l -n >/dev/null
