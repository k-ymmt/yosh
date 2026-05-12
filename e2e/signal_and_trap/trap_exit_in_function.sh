#!/bin/sh
# POSIX_REF: 2.14.14 trap
# DESCRIPTION: EXIT trap set in function fires at shell exit
# EXPECT_OUTPUT<<END
# hello
# goodbye
# END
setup() {
  trap 'echo goodbye' EXIT
}
setup
echo hello
