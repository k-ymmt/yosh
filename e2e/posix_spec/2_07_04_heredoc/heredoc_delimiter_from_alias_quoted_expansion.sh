#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: A here-document delimiter inside an alias value keeps its expansion text after quote removal ($x, $(...), $((...)), backquotes, ~)
# EXPECT_OUTPUT<<END
# OK
# one $x
# two
# three $x
# END
# EXPECT_EXIT: 0
x=zz
alias h='cat <<"END$x"'
if false; then
  h
body
END$x
fi
echo OK
alias g='cat <<"E$x$(echo q)$((1+2))`b`~"'
g
one $x
E$x$(echo q)$((1+2))`b`~
alias k='cat <<E\$y'
k
two
E$y
alias m='cat <<-  "T  $x"'
m
	three $x
	T  $x
