#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: A first word expanding to nothing makes the next word the command
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
e=
$e echo hi
