#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: Use case - option parsing with getopts including an option argument
# EXPECT_OUTPUT: verbose=1 out=build.log rest=src.c
set -- -v -o build.log src.c
verbose=0
out=""
while getopts vo: opt; do
  case $opt in
    v) verbose=1 ;;
    o) out=$OPTARG ;;
    ?) exit 2 ;;
  esac
done
shift $((OPTIND - 1))
echo "verbose=$verbose out=$out rest=$1"
