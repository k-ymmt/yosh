#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: eval and command substitution report a lexical error on the first token as a syntax error
# EXPECT_OUTPUT<<END
# 2
# 2
# 2
# END
# EXPECT_EXIT: 0
(eval '"abc') 2>/dev/null
echo "$?"
(eval '${x') 2>/dev/null
echo "$?"
(eval 'x=$(${x)') 2>/dev/null
echo "$?"
