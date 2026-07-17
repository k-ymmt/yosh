#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Use case - basename/dirname/extension handling with pure parameter expansion
# EXPECT_OUTPUT<<END
# base=tool.tar.gz
# dir=/usr/local/bin
# noext=tool.tar
# ext=gz
# END
p=/usr/local/bin/tool.tar.gz
base=${p##*/}
dir=${p%/*}
echo "base=$base"
echo "dir=$dir"
echo "noext=${base%.*}"
echo "ext=${base##*.}"
