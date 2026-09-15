# command substitution of a builtin (fork-heavy)
i=0
while [ "$i" -lt 2000 ]; do
    x=$(echo "$i")
    i=$((i + 1))
done
echo "$x"
