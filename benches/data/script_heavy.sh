#!/bin/sh
# script_heavy.sh — W2 workload for performance measurement.
# Exercises Lexer, Parser, Expander, and Executor hot paths.

# ── Section A: for-loop with arithmetic ─────────────────────────────────
SUM=0
for i in $(seq 1 1000); do
    SUM=$((SUM + i))
done
echo "sum=$SUM"

# ── Section B: function defined once, called 1000 times ─────────────────
greet() {
    name=$1
    echo "hello, $name"
}

i=0
while [ "$i" -lt 1000 ]; do
    greet "world" > /dev/null
    i=$((i + 1))
done

# ── Section C: parameter expansion variety ──────────────────────────────
VAR="hello world"
UNSET=""
for _ in $(seq 1 200); do
    : "${UNSET:-fallback}"
    : "${VAR#hello }"
    : "${VAR%world}"
    : "${#VAR}"
    : "$(echo "$VAR")"
done

# ── Section D: redirection ──────────────────────────────────────────────
TMP=$(mktemp)
echo "line one" > "$TMP"
echo "line two" >> "$TMP"
echo "line three to stderr" 1>&2 2>/dev/null

cat <<HEREDOC > "$TMP"
heredoc body
more heredoc body
HEREDOC

rm -f "$TMP"
