#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: The closing ) of $(...) is found by parsing the body: escaped/quoted parens, nested quotes, case patterns, comments and here-documents
# EXPECT_OUTPUT<<END
# )
# (
# a)
# )
# A
# it's
# c
# a)b ( sub
# 6
# h ) B x)
# END
x=$(echo \)); echo "$x"
echo $(echo \( )
echo $(echo "$(echo "a)")")
echo $(echo ${x:-\)})
echo $(case a in a) echo A;; esac)
x=$(cat <<EOF
it's
EOF
)
echo "$x"
y=$(
  # it's a comment
  echo c
)
echo "$y"
echo "$(echo 'a)b')" $(echo "(" ) $( (echo sub) )
echo $(( $(case 1 in 1) echo 5;; esac) + 1 ))
cat <<E
h $(echo \)) $(case b in b) echo B;; esac) ${u:-$(echo "x)")}
E
