#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Pipeline exit status is that of the last command
# EXPECT_OUTPUT<<END
# 0
# 1
# END
true | true
echo "$?"
true | false
echo "$?"
