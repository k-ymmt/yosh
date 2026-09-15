# output-heavy echo/printf
i=0
while [ "$i" -lt 20000 ]; do
    echo "line $i: some text here"
    printf '%s=%d\n' "key$i" "$i"
    i=$((i + 1))
done
