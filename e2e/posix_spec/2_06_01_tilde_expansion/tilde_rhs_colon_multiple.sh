#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Each tilde after an unquoted ':' in an assignment expands
# EXPECT_OUTPUT: /home/x/a:/home/x/b
# EXPECT_EXIT: 0
HOME=/home/x
PATH=~/a:~/b
echo "$PATH"
