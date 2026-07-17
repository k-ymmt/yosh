#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: Use case - read two-column records and track the maximum value
# EXPECT_OUTPUT: winner=carol score=92
best_name=""
best_score=-1
while read -r name score; do
  if [ "$score" -gt "$best_score" ]; then
    best_name=$name
    best_score=$score
  fi
done <<EOF
alice 85
bob 78
carol 92
dave 90
EOF
echo "winner=$best_name score=$best_score"
