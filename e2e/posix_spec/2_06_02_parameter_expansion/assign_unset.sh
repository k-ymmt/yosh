#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:=word} assigns word to var when var is unset
# EXPECT_OUTPUT<<END
# hello
# hello
# END
# EXPECT_EXIT: 0
unset x
echo "${x:=hello}"
echo "$x"
