#!/bin/sh
# POSIX_REF: 2.14 test
# DESCRIPTION: -h and -L both detect symbolic links
# EXPECT_OUTPUT: h L
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
target="$TEST_TMPDIR/sym_target_$$"
link="$TEST_TMPDIR/sym_link_$$"
: > "$target"
ln -s "$target" "$link"
[ -h "$link" ] && printf 'h '
[ -L "$link" ] && printf 'L'
echo
rm -f "$link" "$target"
