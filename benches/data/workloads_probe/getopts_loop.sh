# getopts parsing in a loop
i=0
while [ "$i" -lt 3000 ]; do
    OPTIND=1
    while getopts "ab:c" opt -a -b val -c; do :; done
    i=$((i + 1))
done
echo "$OPTIND"
