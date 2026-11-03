#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: Aliases are substituted inside $(...) and backquoted bodies, here-documents and arithmetic, with the aliases in effect when the enclosing command is parsed
# EXPECT_OUTPUT<<END
# inner
# bq
# dq bq2
# nested
# here
# 3
# dflt
# x
# echo
# END
# EXPECT_EXIT: 0
alias e=echo
x=$(e inner); echo $x
x=`e bq`; echo $x
echo "$(e dq) `e bq2`"
echo $(e $(e nested))
cat <<EOF
$(e here)
EOF
echo $((1 + $(e 2)))
echo ${unset_var:-$(e dflt)}
alias a='e $(e x)'
a
# The body is parsed with the enclosing command, before the alias
# command inside it runs.
echo $(alias e=printf; e echo)
