#!/bin/sh
# POSIX_REF: 8 Environment Variables - OLDPWD
# DESCRIPTION: cd sets OLDPWD to the prior PWD
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/x"
prev="$PWD"
cd "$TEST_TMPDIR/x"
case "$OLDPWD" in "$prev") exit 0 ;; *) exit 1 ;; esac
