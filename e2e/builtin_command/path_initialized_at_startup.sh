#!/bin/sh
# POSIX_REF: 8. Environment Variables (PATH)
# DESCRIPTION: yosh sets a non-empty default PATH at startup when environment has none
# EXPECT_EXIT: 0
# Run a yosh sub-invocation with env -i so PATH is truly absent from its
# inherited environment; yosh should populate a default so `sh` is findable.
# The outer (current) yosh sets PATH=/bin:/usr/bin so `env` and the yosh
# binary itself are resolvable.
PATH=/bin:/usr/bin
env -i ./target/debug/yosh -c '
  case "$PATH" in
    "" ) exit 1 ;;
  esac
  command -v sh >/dev/null 2>&1 || exit 1
  exit 0
'
