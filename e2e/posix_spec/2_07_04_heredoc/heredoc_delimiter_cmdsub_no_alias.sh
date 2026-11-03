#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: A command substitution spelled in a here-document delimiter is not alias-substituted (quote removal only)
# EXPECT_OUTPUT<<END
# body
# alias body
# ok
# END
# EXPECT_EXIT: 0
alias a='fi'
alias h='cat <<"$(a)"'
if false; then
cat <<"$(a)"
skipped
$(a)
fi
cat <<"$(a)"
body
$(a)
eval 'h
alias body
$(a)'
echo ok
