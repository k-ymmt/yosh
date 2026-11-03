#!/bin/sh
# POSIX_REF: 2.10.2 Shell Grammar Rules (pipeline: Bang pipe_sequence)
# DESCRIPTION: `!` is only valid before a whole pipeline, so `a | ! b` and `! ! a` are syntax errors
# EXPECT_OUTPUT<<END
# 2
# 2
# 1
# END
# EXPECT_EXIT: 0
(eval 'echo a | ! cat') 2>/dev/null
echo "$?"
(eval '! ! true') 2>/dev/null
echo "$?"
! true
echo "$?"
