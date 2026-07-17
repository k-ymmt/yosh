#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Use case - FizzBuzz combining a counter loop, arithmetic, and branching
# EXPECT_OUTPUT<<END
# 1
# 2
# Fizz
# 4
# Buzz
# Fizz
# 7
# 8
# Fizz
# Buzz
# 11
# Fizz
# 13
# 14
# FizzBuzz
# END
i=1
while [ "$i" -le 15 ]; do
  if [ $((i % 15)) -eq 0 ]; then
    echo FizzBuzz
  elif [ $((i % 3)) -eq 0 ]; then
    echo Fizz
  elif [ $((i % 5)) -eq 0 ]; then
    echo Buzz
  else
    echo "$i"
  fi
  i=$((i + 1))
done
