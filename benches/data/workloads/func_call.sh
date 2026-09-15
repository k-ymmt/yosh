# function calls with positional params and local assignment
f() { a=$1; b=$2; : "$a$b"; }
i=0
while [ "$i" -lt 20000 ]; do
    f one two
    i=$((i + 1))
done
echo "$i"
