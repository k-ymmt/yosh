# pathname expansion over a directory with many files
d=$(mktemp -d)
i=0
while [ "$i" -lt 3000 ]; do : > "$d/f$i.txt"; i=$((i + 1)); done
j=0
while [ "$j" -lt 20 ]; do
    set -- "$d"/*.txt
    n=$#
    set -- "$d"/f1*.txt
    m=$#
    j=$((j + 1))
done
rm -rf "$d"
echo "$n $m"
