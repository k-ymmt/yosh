# many positional params + shift loop + "$@" / $# use
set -- $(seq 1 5000)
n=0
while [ $# -gt 0 ]; do
    n=$((n + $1))
    shift
done
echo "$n"
