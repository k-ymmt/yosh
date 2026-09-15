# test builtin with string and file operators
i=0; n=0
while [ "$i" -lt 20000 ]; do
    if [ "$i" -eq 5 ] || [ "x$i" = "x7" ] || [ -n "$i" ] && [ ! -d "/nonexistent$i" ]; then
        n=$((n + 1))
    fi
    i=$((i + 1))
done
echo "$n"
