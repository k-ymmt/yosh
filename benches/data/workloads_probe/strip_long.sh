# prefix/suffix removal on a long string
s=$(seq 1 3000 | tr '\n' /)
i=0
while [ "$i" -lt 300 ]; do
    a="${s%%/*}"; b="${s##*/}"; c="${s%/*}"; d="${s#*/}"
    i=$((i + 1))
done
echo "${#a} ${#b} ${#c} ${#d}"
