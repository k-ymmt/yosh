#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Use case - take only the first N lines of a stream with head
# EXPECT_OUTPUT<<END
# line1
# line2
# END
printf 'line%d\n' 1 2 3 4 5 | head -n 2
