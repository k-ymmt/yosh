#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc with no operands edits the previous command (requires editor; should not crash)
# XFAIL: harness limitation (fc editor invocation needs an interactive context)
# EXPECT_EXIT: 0
fc 2>&1 >/dev/null </dev/null
