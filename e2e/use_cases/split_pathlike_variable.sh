#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Use case - split a PATH-like colon-separated variable into entries
# EXPECT_OUTPUT<<END
# entry: /usr/local/bin
# entry: /usr/bin
# entry: /bin
# END
searchpath="/usr/local/bin:/usr/bin:/bin"
old_ifs=$IFS
IFS=:
for dir in $searchpath; do
  echo "entry: $dir"
done
IFS=$old_ifs
