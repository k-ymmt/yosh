#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: The same expression text evaluated repeatedly follows the current variable values and side-effect rules
# EXPECT_OUTPUT<<END
# 0:0:1 1:2:2 2:4:1
# 3 9
# 7 9
# 9 1
# 6 6
# 4
# END
out=
i=0
while [ "$i" -lt 3 ]; do
    out="$out $i:$((i * 2)):$((i ? 2 / i : 1))"
    i=$((i + 1))
done
echo $out
c=0
for k in 1 2 3; do : $((c += 1)) $((k == 3 ? (d = c * 3) : 0)); done
echo "$c $d"
for k in 0 1; do : $((k && (c = 7))); done
echo "$c $d"
e='d'
for k in 1; do echo "$((e)) $((k || (c = 1)))"; done
e='a * 2'
a=3
for k in 1 2; do r=$((e)); done
echo "$r $((e))"
x=1+1
echo $((x * 2))
