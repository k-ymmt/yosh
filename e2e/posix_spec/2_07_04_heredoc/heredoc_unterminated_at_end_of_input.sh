#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: End of input ends a here-document whose delimiter line is missing (script, eval, dot), as in dash and bash
# EXPECT_OUTPUT<<END
# evalbody|
# dotbody
# rc=0
# a
# same-line
# last
# END
# EXPECT_EXIT: 0
eval 'cat <<E
evalbody'; echo '|'
tmp=${TMPDIR:-/tmp}/yosh_hd_eof_$$
printf 'cat <<E\ndotbody\n' > "$tmp"
. "$tmp"
echo rc=$?
rm -f "$tmp"
cat <<A; echo same-line
a
A
cat <<E
last
