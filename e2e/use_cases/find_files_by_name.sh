#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Use case - locate files in a directory tree with find and sort the result
# EXPECT_OUTPUT<<END
# ./docs/guide.md
# ./readme.md
# END
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
mkdir docs src
: > readme.md
: > docs/guide.md
: > src/main.c
find . -name '*.md' | LC_ALL=C sort
