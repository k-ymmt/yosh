# large here-document with expansion, consumed by a builtin loop
x=val
i=0
while [ "$i" -lt 200 ]; do
    n=0
    while read -r line; do n=$((n + 1)); done <<EOF2
line $x 1
line $x 2
line $x 3
line $x 4
line $x 5
line $x 6
line $x 7
line $x 8
line $x 9
line $x 10
EOF2
    i=$((i + 1))
done
echo "$n"
