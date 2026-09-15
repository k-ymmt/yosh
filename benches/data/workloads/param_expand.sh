# parameter expansion variety
VAR="hello world this is a test string"
UNSET=""
i=0
while [ "$i" -lt 10000 ]; do
    : "${UNSET:-fallback}" "${VAR#hello }" "${VAR%string}" "${#VAR}" "${VAR##* }" "${VAR%% *}"
    x="${VAR:-x}${VAR:+y}"
    i=$((i + 1))
done
echo "$x"
