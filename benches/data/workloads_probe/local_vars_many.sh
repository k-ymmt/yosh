# many variables in the environment and repeated lookups
i=0
i=0
while [ "$i" -lt 2000 ]; do eval "var_$i=$i"; i=$((i + 1)); done
j=0; s=0
while [ "$j" -lt 20000 ]; do
    s=$((s + var_1999 + var_0))
    j=$((j + 1))
done
echo "$s"
