#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: ${10} ${11} — multi-digit positional parameters require braces
# EXPECT_OUTPUT<<END
# ten
# eleven
# END
set -- a b c d e f g h i ten eleven
echo "${10}"
echo "${11}"
