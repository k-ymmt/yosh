#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -e EDITOR picks the editor for the edit step
# XFAIL: harness limitation (fc -e relies on launching an editor)
# EXPECT_EXIT: 0
fc -e cat 2>&1 >/dev/null </dev/null
