#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting (interaction with 2.6.6 Pathname Expansion)
# DESCRIPTION: Literal * still triggers pathname expansion under IFS=:
# EXPECT_OUTPUT: a.tmpext
# EXPECT_EXIT: 0
IFS=:
d=$(mktemp -d) || exit 1
trap 'rm -rf "$d"' EXIT
touch "$d/a.tmpext"
cd "$d"
echo *.tmpext
