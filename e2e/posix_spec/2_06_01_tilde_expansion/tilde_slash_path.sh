#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: '~/path' expands to $HOME/path
# EXPECT_OUTPUT: /tmp/hdir/bin
# EXPECT_EXIT: 0
HOME=/tmp/hdir
echo ~/bin
