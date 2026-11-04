#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: The dot diagnostic for a missing file prints the path's original bytes, not a UTF-8 replacement character
# EXPECT_OUTPUT: dot missing ok
# EXPECT_EXIT: 0
# (A file whose name is not UTF-8 cannot be created on every file system
# — APFS refuses it — so only the missing-file path is exercised.)
cd "$TEST_TMPDIR" || exit 1
ff=$(printf '\377')
bad=$(printf '\357\277\275')
( . "/nonexistent/$ff" ) 2>err
if grep -q "/nonexistent/$ff:" err && ! grep -q "$bad" err; then
  echo "dot missing ok"
else
  od -c err
fi
