#!/bin/sh
# POSIX_REF: 2.14.5 continue
# DESCRIPTION: continue N skips to Nth enclosing loop
# EXPECT_OUTPUT<<END
# 1-a
# 2-a
# 3-a
# END
for i in 1 2 3; do
  for j in a b c; do
    echo "${i}-${j}"
    continue 2
  done
done
