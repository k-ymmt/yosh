#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_NUMERIC
# DESCRIPTION: LC_NUMERIC is exported to child processes unchanged
# EXPECT_EXIT: 0
command -v /usr/bin/printf >/dev/null || exit 0
out=$(LC_NUMERIC=de_DE.UTF-8 /usr/bin/printf '%.2f' 1234.5)
case "$out" in 1234.50|1234,50) exit 0 ;; *) exit 1 ;; esac
