# pipelines of builtins (fork per stage)
i=0
while [ "$i" -lt 1000 ]; do
    echo "$i" | { read -r x; :; }
    i=$((i + 1))
done
echo "$i"
