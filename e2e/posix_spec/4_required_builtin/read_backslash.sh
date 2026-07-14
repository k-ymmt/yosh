#!/bin/sh
# POSIX_REF: read utility
# DESCRIPTION: read without -r joins backslash-newline continuations and unescapes backslashes
# EXPECT_OUTPUT<<END
# x=ab
# y=ptq
# END
# EXPECT_EXIT: 0
printf 'a\\\nb\n' > "$TEST_TMPDIR/in1"
read x < "$TEST_TMPDIR/in1"
echo "x=$x"
printf 'p\\tq\n' > "$TEST_TMPDIR/in2"
read y < "$TEST_TMPDIR/in2"
echo "y=$y"
