#!/bin/sh
# POSIX_REF: 2.9.4.5 case Conditional Construct
# DESCRIPTION: Use case - filter log lines by severity using while read and case
# EXPECT_OUTPUT<<END
# 2024-01-01 ERROR disk full
# 2024-01-02 ERROR network down
# END
while IFS= read -r line; do
  case $line in
    *ERROR*) echo "$line" ;;
  esac
done <<EOF
2024-01-01 INFO service started
2024-01-01 ERROR disk full
2024-01-02 WARN memory low
2024-01-02 ERROR network down
2024-01-03 INFO service stopped
EOF
