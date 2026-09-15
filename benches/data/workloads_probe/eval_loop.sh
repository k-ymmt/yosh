# eval re-parsing in a loop
i=0
while [ "$i" -lt 10000 ]; do
    eval "v$((i % 10))=\$i; : \"\$v$((i % 10))\""
    i=$((i + 1))
done
echo "$v9"
