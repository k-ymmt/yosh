#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Caller positional parameters restored after function call
# EXPECT_OUTPUT<<END
# inner-a inner-b
# x y z
# END
myfunc() {
  echo "$1 $2"
}
set -- x y z
myfunc inner-a inner-b
echo "$1 $2 $3"
