#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Deeply nested parentheses evaluate, and absurd nesting is an ordinary arithmetic error, not a crash
# EXPECT_OUTPUT<<END
# 1
# survived
# END
# EXPECT_EXIT: 0
nest() {
    o= c= i=0
    while [ "$i" -lt "$1" ]; do o="$o(" c="$c)" i=$((i + 1)); done
    e="${o}1${c}"
}
nest 200
echo $(( $e ))
nest 1000
(echo $(( $e ))) 2>/dev/null
echo survived
