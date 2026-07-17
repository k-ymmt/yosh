#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Use case - deduplicate a list of values with sort | uniq
# EXPECT_OUTPUT<<END
# apple
# banana
# cherry
# END
printf '%s\n' banana apple cherry banana apple | LC_ALL=C sort | uniq
