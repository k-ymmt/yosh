#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Use case - argument validation printing usage to stderr with exit 2
# EXPECT_EXIT: 2
# EXPECT_STDERR: usage: mytool FILE
main() {
  if [ $# -lt 1 ]; then
    echo "usage: mytool FILE" >&2
    return 2
  fi
  echo "processing $1"
}
main
exit $?
