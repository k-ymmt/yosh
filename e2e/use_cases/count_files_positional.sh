#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: Use case - count files matching a glob via set -- and $#
# EXPECT_OUTPUT: 3 txt files
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
: > a.txt
: > b.txt
: > c.txt
: > d.log
set -- *.txt
echo "$# txt files"
