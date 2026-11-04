#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: A trapped signal that ends a waiting read still assigns the partial line read so far, like end-of-file (status 1)
# EXPECT_OUTPUT<<END
# 1 <partial>
# 1 <part\ial>
# 1 <a> <b> <c d>
# 1 <part>
# 1 <>
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
mkfifo idle || exit 1
# Opened read-write: the fifo never reports end-of-file, so read waits
# after the bytes written below until a signal ends it.
exec 3<>idle
trap : USR1
# Send USR1 every second until stop_nudge. A signal that comes before
# read waits only runs the trap (or is put off while read has input
# ready); a later one ends the read. So the result does not depend on
# scheduling, and a slow start cannot leave read waiting forever.
nudge() {
  rm -f marker
  (
    while sleep 1; [ ! -e marker ]; do
      kill -USR1 $$ || exit
    done
  ) >/dev/null 2>&1 &
  nudger=$!
}
# Stop nudging, and wait until no signal can still be on its way. A
# trapped signal cuts wait short, hence the loop.
stop_nudge() {
  : >marker
  while kill -0 "$nudger" 2>/dev/null; do wait "$nudger"; done
}
x=old
printf 'partial' >&3
nudge
read x <&3
echo "$? <$x>"
stop_nudge
printf 'part\\ial' >&3
nudge
read -r x <&3
echo "$? <$x>"
stop_nudge
# The usual splitting applies to the partial line.
x=old y=old z=old
printf ' a  b c d ' >&3
nudge
read x y z <&3
echo "$? <$x> <$y> <$z>"
stop_nudge
# A pending backslash escape is dropped, as at end-of-file.
printf 'part\\' >&3
nudge
read x <&3
echo "$? <$x>"
stop_nudge
# Nothing read yet: the variable is set to the empty string.
x=old
nudge
read x <&3
echo "$? <$x>"
stop_nudge
