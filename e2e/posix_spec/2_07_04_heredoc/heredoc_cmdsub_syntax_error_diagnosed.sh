#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: A syntax error in a command substitution inside a here-document body is diagnosed, not silently dropped
# EXPECT_OUTPUT<<END
# a  b
# c  d
# diagnosed
# END
# EXPECT_EXIT: 0
cat <<E 2>/dev/null
a $(fi) b
E
cat <<E 2>/dev/null
c `fi` d
E
err=$( { cat <<E >/dev/null; } 2>&1
$(fi)
E
)
case $err in *fi*) echo diagnosed;; *) echo "silent: [$err]";; esac
