#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: An alias value containing a newline is substituted in full, and the rest of the line continues after it
# EXPECT_OUTPUT<<END
# 1
# 2
# 1
# 2
# 3
# END
# EXPECT_EXIT: 0
alias ml='echo 1
echo 2'
ml
ml; echo 3
