# external command spawn
i=0
while [ "$i" -lt 1000 ]; do
    /usr/bin/true
    i=$((i + 1))
done
echo "$i"
