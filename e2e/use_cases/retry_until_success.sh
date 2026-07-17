#!/bin/sh
# POSIX_REF: 2.9.4.4 until Loop
# DESCRIPTION: Use case - retry loop that keeps attempting until an operation succeeds
# EXPECT_OUTPUT<<END
# attempt 1
# attempt 2
# attempt 3
# succeeded after 3 attempts
# END
attempts=0
# Simulated flaky operation: succeeds on the third try
flaky_op() {
  [ "$attempts" -ge 3 ]
}
until flaky_op; do
  attempts=$((attempts + 1))
  echo "attempt $attempts"
done
echo "succeeded after $attempts attempts"
