#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde with username resolves via getpwnam when user exists
# EXPECT_EXIT: 0
x=~root/suffix
case "$x" in
    /*/suffix) exit 0 ;;
    '~root/suffix') exit 0 ;;
    *) echo "unexpected: $x" >&2; exit 1 ;;
esac
