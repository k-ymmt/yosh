#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd -P resolves the physical path
# EXPECT_EXIT: 0
cd -P /tmp
# On Linux, /tmp is already physical; on macOS, /tmp -> /private/tmp.
# Accept either.
case "$PWD" in
    /tmp|/private/tmp) exit 0 ;;
    *) echo "unexpected PWD: $PWD" >&2; exit 1 ;;
esac
