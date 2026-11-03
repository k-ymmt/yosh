#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Unquoted glob characters in the word of ${x:-w} undergo pathname expansion; quoted ones do not
# EXPECT_OUTPUT<<END
# f1 f2
# f*
# END
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/g"
cd "$TEST_TMPDIR/g" || exit 1
: > f1
: > f2
unset x
echo ${x:-f*}
echo ${x:-"f*"}
