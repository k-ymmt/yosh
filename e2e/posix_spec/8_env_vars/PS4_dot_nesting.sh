#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: PS4 first character repeats inside a dot-sourced script
# EXPECT_STDERR: ++ echo sourced
# EXPECT_OUTPUT: sourced
# EXPECT_EXIT: 0
PS4='+ '
d=$(mktemp) || exit 1
echo 'echo sourced' > "$d"
set -x
. "$d"
rm -f "$d"
