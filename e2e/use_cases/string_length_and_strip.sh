#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Use case - string length and prefix/suffix stripping for version parsing
# EXPECT_OUTPUT<<END
# len=9
# major=1
# rest=2.3-rc1
# version=1.2.3
# END
tag="release-1.2.3-rc1"
ver=${tag#release-}
echo "len=${#ver}"
echo "major=${ver%%.*}"
echo "rest=${ver#*.}"
echo "version=${ver%-rc*}"
