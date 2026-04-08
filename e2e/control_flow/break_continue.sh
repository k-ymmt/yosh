#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: break exits loop, continue skips to next iteration
# EXPECT_OUTPUT<<END
# 1
# 1
# 3
# END
for i in 1 2 3; do
  if test "$i" = 2; then break; fi
  echo "$i"
done
for i in 1 2 3; do
  if test "$i" = 2; then continue; fi
  echo "$i"
done
