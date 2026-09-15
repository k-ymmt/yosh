# IFS-based splitting of a large string with custom IFS
s=$(seq 1 50000 | tr '\n' ,)
IFS=,
set -- $s
echo "$#"
IFS=' '
n=0
for w in $s; do n=$((n + 1)); done
echo "$n"
