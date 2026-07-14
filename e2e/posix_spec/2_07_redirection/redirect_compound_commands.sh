#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: Redirections apply to while loops, brace groups, and if statements
# EXPECT_OUTPUT<<END
# got l1
# got l2
# b1
# b2
# yes
# END
# EXPECT_EXIT: 0
printf 'l1\nl2\n' > "$TEST_TMPDIR/in"
while read l; do echo "got $l"; done < "$TEST_TMPDIR/in"
{ echo b1; echo b2; } > "$TEST_TMPDIR/f1"
cat "$TEST_TMPDIR/f1"
if true; then echo yes; fi > "$TEST_TMPDIR/f2"
cat "$TEST_TMPDIR/f2"
