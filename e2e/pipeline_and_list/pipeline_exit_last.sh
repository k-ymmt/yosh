#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Pipeline exit status is from the last command
# EXPECT_OUTPUT<<END
# 0
# 1
# END
false | true
echo "$?"
true | false
echo "$?"
