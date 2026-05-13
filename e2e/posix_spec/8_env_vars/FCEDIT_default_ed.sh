#!/bin/sh
# POSIX_REF: 8 Environment Variables - FCEDIT
# DESCRIPTION: when FCEDIT is unset, fc uses ed by default
# XFAIL: harness limitation (fc invokes an editor; cannot test non-interactively)
# EXPECT_EXIT: 0
unset FCEDIT
fc 2>&1 >/dev/null </dev/null
