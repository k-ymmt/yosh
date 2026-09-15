# for loop over a word list with field splitting
list=""
i=0
while [ "$i" -lt 200 ]; do list="$list w$i"; i=$((i + 1)); done
n=0
j=0
while [ "$j" -lt 100 ]; do
    for w in $list; do
        n=$((n + 1))
    done
    j=$((j + 1))
done
echo "$n"
