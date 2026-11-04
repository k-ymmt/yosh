#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -v, command -V and type print a non-UTF-8 PATH directory with its original bytes, not U+FFFD
# EXPECT_OUTPUT: raw bytes ok
# EXPECT_EXIT: 0
# (APFS refuses non-UTF-8 file names; where the directory cannot be
# created the check is skipped.)
cd "$TEST_TMPDIR" || exit 1
ff=$(printf '\377')
bad=$(printf '\357\277\275')
if ! mkdir "d$ff" 2>/dev/null; then
  echo "raw bytes ok"
  exit 0
fi
printf '#!/bin/sh\n' > "d$ff/rawtool"
chmod +x "d$ff/rawtool"
PATH="$TEST_TMPDIR/d$ff:/usr/bin:/bin"
{ command -v rawtool; command -V rawtool; type rawtool; } > out
if [ "$(grep -c "/d$ff/rawtool" out)" = 3 ] && ! grep -q "$bad" out; then
  echo "raw bytes ok"
else
  od -c out
fi
