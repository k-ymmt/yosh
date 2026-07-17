#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters - @
# DESCRIPTION: Use case - forward arguments with "$@" preserving embedded spaces
# EXPECT_OUTPUT<<END
# arg=[first]
# arg=[second part]
# arg=[third]
# count=3
# END
print_args() {
  for arg in "$@"; do
    echo "arg=[$arg]"
  done
  echo "count=$#"
}
set -- first "second part" third
print_args "$@"
