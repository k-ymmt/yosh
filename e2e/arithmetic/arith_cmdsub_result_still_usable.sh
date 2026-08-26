#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Command substitution result (trailing newline stripped) remains a valid arithmetic operand
# EXPECT_OUTPUT<<END
# 3
# 3
# END
echo "$(( $(echo 2) + 1 ))"
echo "$(( `echo 2` + 1 ))"
