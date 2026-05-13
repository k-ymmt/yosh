#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: export NAME= sets exported empty string
# EXPECT_OUTPUT: <>
# EXPECT_EXIT: 0
export foo=
sh -c 'echo "<$foo>"'
