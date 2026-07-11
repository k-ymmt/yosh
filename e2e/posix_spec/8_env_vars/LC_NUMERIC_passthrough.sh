#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_NUMERIC
# DESCRIPTION: LC_NUMERIC is exported to child processes unchanged
# EXPECT_EXIT: 0
command -v /usr/bin/printf >/dev/null || exit 0
# A decimal-free argument parses identically in every locale; only the
# *output* separator varies, which is exactly what passthrough should
# observe. An input like 1234.5 breaks when LC_ALL is unset: printf
# parses the argument under LC_NUMERIC=de_DE (comma decimal), stops at
# the ".", and exits 1 with "not completely converted".
out=$(LC_NUMERIC=de_DE.UTF-8 /usr/bin/printf '%.2f' 1234)
case "$out" in 1234.00|1234,00) exit 0 ;; *) exit 1 ;; esac
