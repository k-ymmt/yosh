#!/bin/sh
# POSIX_REF: 4 Utilities - test
# DESCRIPTION: POSIX.1-2024 -nt, -ot and -ef primaries
# EXPECT_OUTPUT<<END
# 0 1 0 1
# 0 1
# 0 0
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
touch -t 202001010000 old
touch new
ln -s old lnk
test new -nt old; a=$?
test old -nt new; b=$?
[ old -ot new ]; c=$?
[ new -ot old ]; d=$?
echo "$a $b $c $d"
[ old -ef lnk ]; e=$?
[ old -ef new ]; f=$?
echo "$e $f"
test old -nt missing; g=$?
test missing -ot old; h=$?
echo "$g $h"
