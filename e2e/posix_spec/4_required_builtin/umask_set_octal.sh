#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask 022 sets the mask
# EXPECT_OUTPUT: 022
# EXPECT_EXIT: 0
umask 022
out=$(umask)
# Accept both 022 and 0022 (POSIX allows leading zero)
case "$out" in
    022|0022) echo 022 ;;
    *) exit 1 ;;
esac
