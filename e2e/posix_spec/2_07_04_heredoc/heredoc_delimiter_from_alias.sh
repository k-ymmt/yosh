#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: A here-document delimiter produced by alias substitution (after a trailing-blank alias) is the substituted word, not its source spelling
# EXPECT_OUTPUT: body
# EXPECT_EXIT: 0
alias h='cat << '
alias E=END
h E
body
END
