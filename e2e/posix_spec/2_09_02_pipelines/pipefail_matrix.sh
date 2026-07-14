#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: set -o pipefail exit-status matrix with and without !
# EXPECT_OUTPUT<<END
# 0
# 1
# 1
# 0
# 1
# END
# EXPECT_EXIT: 0
set -o pipefail
true | true; echo $?
true | false; echo $?
false | true; echo $?
! false | true; echo $?
! true | true; echo $?
