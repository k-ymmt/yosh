#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Unquoted '~' expands to $HOME
# EXPECT_OUTPUT: /tmp/hdir
# EXPECT_EXIT: 0
HOME=/tmp/hdir
echo ~
