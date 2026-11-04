#!/bin/sh
# POSIX_REF: 2.15 Special Built-In Utilities - break, continue
# DESCRIPTION: break and continue in a while/until condition list act on that loop and do not abort the commands after it
# EXPECT_OUTPUT<<END
# while-break 3 0
# until-continue 3
# if-continue 3
# in 1
# in 2
# out
# outer-break-2
# END
# EXPECT_EXIT: 0
i=0
while i=$((i+1)); [ "$i" -lt 3 ] || break; do :; done
echo "while-break $i $?"
i=0
until i=$((i+1)); [ "$i" -ge 3 ] || continue; [ "$i" -ge 3 ]; do :; done
echo "until-continue $i"
i=0
while i=$((i+1)); if [ "$i" -lt 3 ]; then continue; fi; false; do :; done
echo "if-continue $i"
for x in 1 2; do
  while break; do :; done
  echo "in $x"
done
echo out
for x in 1 2; do
  until break 2; do :; done
  echo "not reached $x"
done
echo outer-break-2
