#!/bin/sh
# POSIX_REF: 2.7.1 Redirecting Input
# DESCRIPTION: < redirects stdin from a file
# EXPECT_OUTPUT: hello from file
echo "hello from file" > "$TEST_TMPDIR/in.txt"
cat < "$TEST_TMPDIR/in.txt"
