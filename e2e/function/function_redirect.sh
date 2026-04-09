#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Redirect applied to function definition
# EXPECT_EXIT: 0
myfunc() {
  echo "to file"
}
myfunc > "$TEST_TMPDIR/output.txt"
result=$(cat "$TEST_TMPDIR/output.txt")
test "$result" = "to file"
