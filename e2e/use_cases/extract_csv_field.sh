#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Use case - extract the second column from CSV data with cut
# EXPECT_OUTPUT<<END
# apple
# banana
# cherry
# END
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
cat > fruits.csv <<EOF
1,apple,red
2,banana,yellow
3,cherry,red
EOF
cut -d, -f2 fruits.csv
