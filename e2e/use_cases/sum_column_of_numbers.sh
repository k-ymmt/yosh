#!/bin/sh
# POSIX_REF: 2.9.4.3 while Loop
# DESCRIPTION: Use case - sum a column of numbers read line by line
# EXPECT_OUTPUT: total=100
sum=0
while IFS= read -r n; do
  sum=$((sum + n))
done <<EOF
10
20
30
40
EOF
echo "total=$sum"
