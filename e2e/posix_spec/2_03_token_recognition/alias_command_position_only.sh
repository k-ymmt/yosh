#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: Aliases are expanded only in command-name position, not as arguments
# EXPECT_OUTPUT<<END
# sub
# x
# END
# EXPECT_EXIT: 0
alias x='echo sub'
x
echo x
