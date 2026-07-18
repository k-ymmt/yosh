#!/bin/sh
# POSIX_REF: 2.13 Shell Execution Environment
# DESCRIPTION: Use case - cd inside a subshell without affecting the caller's directory
# EXPECT_OUTPUT<<END
# inside subshell
# directory unchanged
# END
before=$PWD
(cd "$TEST_TMPDIR" && echo "inside subshell")
if [ "$PWD" = "$before" ]; then
  echo "directory unchanged"
else
  echo "directory changed"
fi
