#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd - prints the new PWD on stdout
# EXPECT_OUTPUT: /tmp
# EXPECT_EXIT: 0
cd /tmp
cd /etc
cd - > "$TEST_TMPDIR/out"
cat "$TEST_TMPDIR/out"
