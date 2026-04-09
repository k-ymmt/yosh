#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $$ holds shell process ID (numeric)
# EXPECT_EXIT: 0
pid=$$
case "$pid" in
  ''|*[!0-9]*) exit 1 ;;
  *) exit 0 ;;
esac
