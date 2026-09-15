# arithmetic while loop with compound expression
i=0; sum=0
while [ "$i" -lt 20000 ]; do
    sum=$((sum + i * 2 % 7))
    i=$((i + 1))
done
echo "$sum"
