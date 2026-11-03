#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: A CDPATH match through an empty (current-directory) entry prints nothing
# EXPECT_OUTPUT: [] ok
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
mkdir -p real cdp
out=$(CDPATH=cdp: cd real)
case $(CDPATH=cdp: cd real && pwd) in */real) r=ok ;; *) r=bad ;; esac
echo "[$out] $r"
