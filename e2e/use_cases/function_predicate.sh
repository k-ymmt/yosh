#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Use case - predicate function used as an if condition in a loop
# EXPECT_OUTPUT<<END
# 1 odd
# 2 even
# 3 odd
# 4 even
# END
is_even() {
  return $(( $1 % 2 ))
}
for n in 1 2 3 4; do
  if is_even "$n"; then
    echo "$n even"
  else
    echo "$n odd"
  fi
done
