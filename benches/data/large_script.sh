#!/bin/sh
# large_script.sh - benchmark data file with diverse shell syntax (~500 lines)

# === Section 1: Variable assignments and basic expansions ===
NAME="Alice"
GREETING="Hello, ${NAME}!"
COUNT=0
MAX=50
PREFIX=/usr/local
BINDIR="${PREFIX}/bin"
LIBDIR="${PREFIX}/lib"
TMPDIR=/tmp/bench_$$

echo "$GREETING"
echo "Binary dir: ${BINDIR}"
echo "Temp dir: ${TMPDIR}"

# Arithmetic expansion
RESULT=$((COUNT + 1))
SQUARE=$((MAX * MAX))
HALF=$((MAX / 2))
MOD=$((MAX % 7))
echo "Result: ${RESULT}, Square: ${SQUARE}, Half: ${HALF}, Mod: ${MOD}"

# Parameter expansion with defaults
UNSET_VAR=""
VAL1="${UNSET_VAR:-default_value}"
VAL2="${NAME:+found}"
VAL3="${UNSET_VAR:=assigned_default}"
echo "VAL1=${VAL1} VAL2=${VAL2} VAL3=${VAL3}"

# String length
LEN=${#NAME}
echo "Length of NAME: ${LEN}"

# Substring extraction
FULL_PATH="/usr/local/bin/kish"
BASE="${FULL_PATH##*/}"
DIR="${FULL_PATH%/*}"
EXT="${FULL_PATH##*.}"
echo "Base: ${BASE}, Dir: ${DIR}, Ext: ${EXT}"

# === Section 2: Functions ===
greet() {
    local who="${1:-world}"
    echo "Hello, ${who}!"
}

add() {
    local a="$1"
    local b="$2"
    echo $((a + b))
}

max_of_two() {
    if [ "$1" -gt "$2" ]; then
        echo "$1"
    else
        echo "$2"
    fi
}

check_file() {
    local path="$1"
    if [ -f "${path}" ]; then
        echo "file: ${path}"
    elif [ -d "${path}" ]; then
        echo "dir: ${path}"
    else
        echo "not found: ${path}"
    fi
}

repeat_str() {
    local str="$1"
    local n="$2"
    local i=0
    local result=""
    while [ "$i" -lt "$n" ]; do
        result="${result}${str}"
        i=$((i + 1))
    done
    echo "$result"
}

greet "benchmark"
add 3 7
max_of_two 10 20
check_file "/etc/hosts"
repeat_str "abc" 3

# === Section 3: Control flow ===
for i in 1 2 3 4 5; do
    echo "Item: $i"
done

for word in hello world foo bar baz; do
    echo "Word: ${word}"
done

j=0
while [ "$j" -lt 5 ]; do
    echo "While: $j"
    j=$((j + 1))
done

k=10
until [ "$k" -le 0 ]; do
    echo "Until: $k"
    k=$((k - 2))
done

# Case statement
for status in ok warning error unknown; do
    case "$status" in
        ok)
            echo "All good"
            ;;
        warning)
            echo "Be careful"
            ;;
        error)
            echo "Something went wrong"
            ;;
        *)
            echo "Unknown: ${status}"
            ;;
    esac
done

# === Section 4: Pipelines and redirects ===
echo "one two three four five" | tr ' ' '\n' | sort | uniq

# Redirect stdout
echo "benchmark output" >/dev/null

# Redirect stderr
ls /nonexistent 2>/dev/null || echo "handled error"

# Append redirect
printf "line1\n" >>/dev/null
printf "line2\n" >>/dev/null

# Here-document
cat <<'EOF'
This is a heredoc
with multiple lines
and $no expansion
EOF

cat <<EOF
Expanded heredoc: ${NAME}
Count: ${COUNT}
EOF

# Here-string
cat <<<"Hello from herestring: ${NAME}"

# Process substitution-style pipelines
echo "alpha beta gamma delta" | while read -r word; do
    echo "Read: ${word}"
done

# === Section 5: Quoting and special characters ===
SPACES="  hello   world  "
NO_QUOTE=$SPACES
DOUBLE_QUOTE="$SPACES"
echo "No quote: ${NO_QUOTE}"
echo "Double quote: ${DOUBLE_QUOTE}"

SPECIAL='$dollar @at #hash %percent'
echo "Special chars: ${SPECIAL}"

NEWLINES="line1
line2
line3"
echo "${NEWLINES}"

TABS="col1	col2	col3"
echo "${TABS}"

# Escape sequences
echo "Tab:\there"
echo "Newline test"
echo 'Single quoted: $no_expand'

# === Section 6: Arrays and IFS ===
IFS=:
PATH_COPY="/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
for dir in $PATH_COPY; do
    echo "Dir: ${dir}"
done
IFS="
"

IFS=,
CSV="alpha,beta,gamma,delta,epsilon"
for field in $CSV; do
    echo "Field: ${field}"
done
IFS="
"

# === Section 7: More arithmetic ===
x=1
while [ "$x" -le 10 ]; do
    sq=$((x * x))
    cube=$((x * x * x))
    echo "${x}: sq=${sq} cube=${cube}"
    x=$((x + 1))
done

A=255
B=$((A & 0xF0))
C=$((A | 0x01))
D=$((A ^ 0xFF))
E=$((1 << 4))
F=$((A >> 2))
echo "A=${A} AND=${B} OR=${C} XOR=${D} LSH=${E} RSH=${F}"

# === Section 2 Variant: More functions ===
trim() {
    local str="$1"
    echo "${str}" | sed 's/^[[:space:]]*//' | sed 's/[[:space:]]*$//'
}

to_upper() {
    echo "$1" | tr '[:lower:]' '[:upper:]'
}

to_lower() {
    echo "$1" | tr '[:upper:]' '[:lower:]'
}

contains() {
    case "$1" in
        *"$2"*) return 0 ;;
        *) return 1 ;;
    esac
}

starts_with() {
    case "$1" in
        "$2"*) return 0 ;;
        *) return 1 ;;
    esac
}

ends_with() {
    case "$1" in
        *"$2") return 0 ;;
        *) return 1 ;;
    esac
}

trim "  hello world  "
to_upper "hello world"
to_lower "HELLO WORLD"
contains "foobar" "oba" && echo "contains ok"
starts_with "foobar" "foo" && echo "starts_with ok"
ends_with "foobar" "bar" && echo "ends_with ok"

# === Section 3 Variant: Nested control flow ===
for outer in a b c; do
    for inner in 1 2 3; do
        echo "${outer}${inner}"
    done
done

i=0
while [ "$i" -lt 3 ]; do
    j=0
    while [ "$j" -lt 3 ]; do
        echo "nested: $i,$j"
        j=$((j + 1))
    done
    i=$((i + 1))
done

for val in 0 1 2 3 4 5 6 7 8 9; do
    case "$val" in
        0|1|2)
            echo "low: $val"
            ;;
        3|4|5|6)
            echo "mid: $val"
            ;;
        7|8|9)
            echo "high: $val"
            ;;
    esac
done

# Break and continue
for i in 1 2 3 4 5 6 7 8 9 10; do
    if [ "$i" -eq 5 ]; then
        continue
    fi
    if [ "$i" -eq 8 ]; then
        break
    fi
    echo "loop: $i"
done

# === Section 4 Variant: Complex pipelines ===
printf "banana\napple\ncherry\napple\nbanana\napple\n" | sort | uniq -c | sort -rn

echo "The quick brown fox jumps over the lazy dog" \
    | tr ' ' '\n' \
    | sort \
    | uniq

seq_output=""
for n in 1 2 3 4 5 6 7 8 9 10; do
    seq_output="${seq_output}${n}
"
done
echo "$seq_output" | while read -r line; do
    [ -z "$line" ] && continue
    echo "line: $line"
done

# === Section 5 Variant: Parameter expansions ===
URL="https://example.com/path/to/resource.html"
SCHEME="${URL%%:*}"
HOST="${URL#*//}"
HOST="${HOST%%/*}"
PATHPART="${URL#*${HOST}}"
FILENAME="${PATHPART##*/}"
EXTPART="${FILENAME##*.}"
NAMEPART="${FILENAME%.*}"

echo "Scheme: ${SCHEME}"
echo "Host: ${HOST}"
echo "Path: ${PATHPART}"
echo "File: ${FILENAME}"
echo "Extension: ${EXTPART}"
echo "Name: ${NAMEPART}"

VERSION="1.23.456"
MAJOR="${VERSION%%.*}"
REST="${VERSION#*.}"
MINOR="${REST%%.*}"
PATCH="${REST#*.}"
echo "Major: ${MAJOR} Minor: ${MINOR} Patch: ${PATCH}"

# Pattern removal
FILENAME2="report_2024_final.tar.gz"
NOEXT="${FILENAME2%.gz}"
NOEXT2="${NOEXT%.tar}"
echo "Without .gz: ${NOEXT}"
echo "Without .tar.gz: ${NOEXT2}"

# === Section 6 Variant: More IFS and field splitting ===
OLD_IFS="$IFS"
IFS="|"
RECORD="field1|field2|field3|field4"
set -- $RECORD
echo "F1=$1 F2=$2 F3=$3 F4=$4"
IFS="$OLD_IFS"

IFS="-"
DATE_STR="2024-01-15"
set -- $DATE_STR
YEAR="$1"
MONTH="$2"
DAY="$3"
IFS="
"
echo "Year=${YEAR} Month=${MONTH} Day=${DAY}"

# === Section 7 Variant: String operations ===
pad_left() {
    local str="$1"
    local width="$2"
    local pad="${3:- }"
    while [ "${#str}" -lt "$width" ]; do
        str="${pad}${str}"
    done
    echo "$str"
}

pad_right() {
    local str="$1"
    local width="$2"
    local pad="${3:- }"
    while [ "${#str}" -lt "$width" ]; do
        str="${str}${pad}"
    done
    echo "$str"
}

join_with() {
    local sep="$1"
    shift
    local result="$1"
    shift
    for part in "$@"; do
        result="${result}${sep}${part}"
    done
    echo "$result"
}

pad_left "42" 6 "0"
pad_right "hello" 10 "."
join_with ", " "alpha" "beta" "gamma" "delta"

# === Section 8: Heredocs with varied content ===
cat <<'SCRIPT'
#!/bin/sh
# Embedded script fragment
for x in $(seq 1 5); do
    echo "embedded: $x"
done
SCRIPT

cat <<TEMPLATE
Name: ${NAME}
Count: ${COUNT}
Max: ${MAX}
Prefix: ${PREFIX}
TEMPLATE

read_heredoc() {
    while IFS= read -r line; do
        echo "heredoc line: ${line}"
    done <<EOF
first line
second line
third line with ${NAME}
EOF
}
read_heredoc

# === Section 9: Conditionals with various tests ===
test_number() {
    local n="$1"
    if [ "$n" -lt 0 ]; then
        echo "negative"
    elif [ "$n" -eq 0 ]; then
        echo "zero"
    elif [ "$n" -lt 10 ]; then
        echo "single digit"
    elif [ "$n" -lt 100 ]; then
        echo "double digit"
    else
        echo "large"
    fi
}

for n in -5 0 7 42 100 999; do
    test_number "$n"
done

# String tests
for s in "" "hello" "Hello" "HELLO" "123" "hello world"; do
    if [ -z "$s" ]; then
        echo "empty string"
    elif [ "$s" = "$s" ]; then
        echo "string: '${s}'"
    fi
done

# Logical operators
check_range() {
    local n="$1"
    local lo="$2"
    local hi="$3"
    if [ "$n" -ge "$lo" ] && [ "$n" -le "$hi" ]; then
        echo "${n} in [${lo},${hi}]"
    else
        echo "${n} out of [${lo},${hi}]"
    fi
}

check_range 5 1 10
check_range 15 1 10
check_range 10 1 10

# === Section 10: export, readonly, local ===
export BENCH_VAR="exported_value"
export BENCH_NUM=42

readonly BENCH_CONST="immutable"
readonly BENCH_PI=3

echo "BENCH_VAR=${BENCH_VAR}"
echo "BENCH_CONST=${BENCH_CONST}"
echo "BENCH_PI=${BENCH_PI}"

scope_test() {
    local local_var="only in function"
    GLOBAL_VAR="set in function"
    echo "Inside: local=${local_var} global=${GLOBAL_VAR}"
}
scope_test
echo "Outside: global=${GLOBAL_VAR}"

# === Section 11: Arithmetic with variables ===
fib() {
    local n="$1"
    local a=0
    local b=1
    local i=0
    while [ "$i" -lt "$n" ]; do
        local tmp=$((a + b))
        a="$b"
        b="$tmp"
        i=$((i + 1))
    done
    echo "$a"
}

for f in 0 1 2 3 4 5 6 7 8 9 10; do
    echo "fib($f) = $(fib $f)"
done

# Factorial
fact() {
    local n="$1"
    if [ "$n" -le 1 ]; then
        echo 1
        return
    fi
    local sub
    sub=$(fact $((n - 1)))
    echo $((n * sub))
}

for f in 1 2 3 4 5 6 7; do
    echo "$f! = $(fact $f)"
done

# GCD
gcd() {
    local a="$1"
    local b="$2"
    while [ "$b" -ne 0 ]; do
        local tmp="$b"
        b=$((a % b))
        a="$tmp"
    done
    echo "$a"
}

echo "gcd(12,8)=$(gcd 12 8)"
echo "gcd(100,75)=$(gcd 100 75)"
echo "gcd(17,13)=$(gcd 17 13)"

# === Section 12: More case statements ===
classify_char() {
    local c="$1"
    case "$c" in
        [0-9]) echo "digit" ;;
        [a-z]) echo "lowercase" ;;
        [A-Z]) echo "uppercase" ;;
        ' ') echo "space" ;;
        '.'|','|'!'|'?') echo "punctuation" ;;
        *) echo "other" ;;
    esac
}

for ch in 5 a Z ' ' '.' '@'; do
    classify_char "$ch"
done

classify_exit() {
    local code="$1"
    case "$code" in
        0)    echo "success" ;;
        1)    echo "general error" ;;
        2)    echo "misuse" ;;
        126)  echo "not executable" ;;
        127)  echo "not found" ;;
        128)  echo "invalid exit arg" ;;
        130)  echo "terminated by ctrl-c" ;;
        *)    echo "exit code: ${code}" ;;
    esac
}

for code in 0 1 2 126 127 128 130 255; do
    classify_exit "$code"
done

# === Section 13: String manipulation functions ===
ltrim() {
    local str="$1"
    local chars="${2:- }"
    while [ "${str#[$chars]}" != "$str" ]; do
        str="${str#[$chars]}"
    done
    echo "$str"
}

rtrim() {
    local str="$1"
    local chars="${2:- }"
    while [ "${str%[$chars]}" != "$str" ]; do
        str="${str%[$chars]}"
    done
    echo "$str"
}

count_chars() {
    local str="$1"
    local char="$2"
    local count=0
    local tmp="${str//${char}/}"
    local removed=$((${#str} - ${#tmp}))
    echo "$removed"
}

replace_first() {
    local str="$1"
    local old="$2"
    local new="$3"
    local before="${str%%${old}*}"
    local after="${str#*${old}}"
    echo "${before}${new}${after}"
}

ltrim "   hello   "
rtrim "   hello   "
replace_first "hello world hello" "hello" "goodbye"

# === Section 14: Pipelines with multiple commands ===
generate_data() {
    for i in 10 3 7 1 9 2 8 4 6 5; do
        echo "$i"
    done
}

generate_data | sort -n
generate_data | sort -rn | head -5

printf "name,age,city\nAlice,30,NYC\nBob,25,LA\nCarol,35,Chicago\n" | while IFS=, read -r name age city; do
    echo "Name=${name} Age=${age} City=${city}"
done

# Word frequency count simulation
words="the quick brown fox jumps over the lazy dog the fox"
for word in $words; do
    echo "$word"
done | sort | uniq -c | sort -rn

# === Section 15: Final cleanup and miscellaneous ===
is_number() {
    case "$1" in
        ''|*[!0-9]*) return 1 ;;
        *) return 0 ;;
    esac
}

is_integer() {
    case "$1" in
        -*)
            case "${1#-}" in
                ''|*[!0-9]*) return 1 ;;
                *) return 0 ;;
            esac
            ;;
        ''|*[!0-9]*) return 1 ;;
        *) return 0 ;;
    esac
}

for val in "42" "-7" "3.14" "" "abc" "0" "007"; do
    if is_integer "$val"; then
        echo "'${val}' is integer"
    else
        echo "'${val}' is not integer"
    fi
done

min_val() {
    if [ "$1" -le "$2" ]; then
        echo "$1"
    else
        echo "$2"
    fi
}

max_val() {
    if [ "$1" -ge "$2" ]; then
        echo "$1"
    else
        echo "$2"
    fi
}

clamp() {
    local val="$1"
    local lo="$2"
    local hi="$3"
    val=$(max_val "$val" "$lo")
    val=$(min_val "$val" "$hi")
    echo "$val"
}

echo "min(3,7)=$(min_val 3 7)"
echo "max(3,7)=$(max_val 3 7)"
echo "clamp(5,1,10)=$(clamp 5 1 10)"
echo "clamp(-5,1,10)=$(clamp -5 1 10)"
echo "clamp(15,1,10)=$(clamp 15 1 10)"

# Done
echo "Benchmark script complete"
