#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Use case - stream command output into a while read loop for per-line processing
# EXPECT_OUTPUT<<END
# 1: red
# 2: green
# 3: blue
# END
printf '%s\n' red green blue | {
  n=0
  while IFS= read -r color; do
    n=$((n + 1))
    echo "$n: $color"
  done
}
