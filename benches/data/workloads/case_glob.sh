# case pattern matching
i=0; n=0
while [ "$i" -lt 20000 ]; do
    case "file$i.txt" in
        *.log) n=$((n + 1));;
        file1*.txt) n=$((n + 2));;
        *[0-9].txt) n=$((n + 3));;
        *) n=$((n + 4));;
    esac
    i=$((i + 1))
done
echo "$n"
