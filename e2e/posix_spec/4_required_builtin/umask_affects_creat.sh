#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask is honored by file creation
# EXPECT_EXIT: 0
umask 077
: > "$TEST_TMPDIR/f"
# Only owner perms should be set; group and other should be empty
perms=$(ls -l "$TEST_TMPDIR/f" | awk '{print $1}')
case "$perms" in
    -rw-------|-rw-------@) exit 0 ;;
    *) exit 1 ;;
esac
