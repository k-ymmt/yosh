# string building and length
s=""
i=0
while [ "$i" -lt 5000 ]; do
    s="$s$i,"
    i=$((i + 1))
done
echo "${#s}"
