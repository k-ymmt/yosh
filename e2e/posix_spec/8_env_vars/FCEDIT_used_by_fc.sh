#!/bin/sh
# POSIX_REF: 8 Environment Variables - FCEDIT
# DESCRIPTION: FCEDIT selects the editor used by fc with no -e option
# XFAIL: harness limitation (fc invokes an editor; cannot test non-interactively)
# EXPECT_EXIT: 0
FCEDIT=cat
fc 2>&1 >/dev/null </dev/null
