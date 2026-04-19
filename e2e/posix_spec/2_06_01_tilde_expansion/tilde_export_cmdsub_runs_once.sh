#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Command substitution in export RHS runs exactly once, not once per expansion pass
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
HOME=/home/x
COUNT_FILE="$TEST_TMPDIR/cmdsub_count"
: > "$COUNT_FILE"
export NAME=$(echo x >> "$COUNT_FILE"; echo value)
# COUNT_FILE should have exactly one line (one cmdsub invocation).
lines=$(wc -l < "$COUNT_FILE" | tr -d ' ')
if [ "$lines" -eq 1 ] && [ "$NAME" = "value" ]; then
    echo ok
else
    echo "regression: cmdsub ran $lines times, NAME=$NAME" >&2
    exit 1
fi
