# capture large command-substitution output (about 1.3 MB) and split it
out=$(seq 1 200000)
echo "${#out}"
i=0
for w in $out; do i=$((i + 1)); done
echo "$i"
