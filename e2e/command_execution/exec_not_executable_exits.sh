#!/bin/sh
# POSIX_REF: 2.14 exec
# DESCRIPTION: exec failure (found but not executable) exits with 126
# EXPECT_EXIT: 126
# EXPECT_STDERR: permission denied
tmp=$(mktemp)
echo data > "$tmp"
chmod 644 "$tmp"
exec "$tmp"
echo survived
