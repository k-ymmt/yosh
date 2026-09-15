# while read loop over a heredoc-generated file
tmp=$(mktemp)
i=0
while [ "$i" -lt 5000 ]; do
    echo "line $i with some words here"
    i=$((i + 1))
done > "$tmp"
n=0
while read -r a b c rest; do
    n=$((n + 1))
    : "$a" "$b" "$c" "$rest"
done < "$tmp"
rm -f "$tmp"
echo "$n"
