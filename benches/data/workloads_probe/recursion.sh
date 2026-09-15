# recursive function calls (fib 18)
fib() {
    if [ "$1" -lt 2 ]; then echo "$1"; return; fi
    a=$(fib $(($1 - 1))); b=$(fib $(($1 - 2)))
    echo $((a + b))
}
fib2() {
    if [ "$1" -lt 2 ]; then r=$1; return; fi
    fib2 $(($1 - 1)); x=$r
    fib2 $(($1 - 2)); r=$((x + r))
}
fib2 20
echo "$r"
