#!/bin/sh
# POSIX_REF: 2.14.13 shift
# DESCRIPTION: Use case - hand-rolled option parsing with while/case/shift
# EXPECT_OUTPUT: verbose=1 out=result.txt args=input.txt
set -- -v -o result.txt input.txt
verbose=0
out=""
while [ $# -gt 0 ]; do
  case $1 in
    -v) verbose=1; shift ;;
    -o) out=$2; shift 2 ;;
    --) shift; break ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) break ;;
  esac
done
echo "verbose=$verbose out=$out args=$*"
