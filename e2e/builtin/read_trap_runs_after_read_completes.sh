#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: The trap action of a signal that ends a waiting read runs after the read has assigned the partial line and set its status
# EXPECT_OUTPUT<<END
# <timeout>
# seen=<partial> status=1 x=<partial>
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
mkfifo idle || exit 1
# Opened read-write: the fifo never reports end-of-file, so read waits
# after the bytes written below until a signal ends it.
exec 3<>idle
# Send signal $1 every second until stop_nudge. A signal that comes
# before read waits only runs the trap (or is put off while read has
# input ready); a later one ends the read. The trap actions below give
# the same result however often they run, so the outcome does not
# depend on scheduling, and read cannot wait forever.
nudge() {
  rm -f marker
  (
    while sleep 1; [ ! -e marker ]; do
      kill -"$1" $$ || exit
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
# The action's assignment is not overwritten by the read's.
x=old
trap 'x=timeout' ALRM
printf 'partial' >&3
nudge ALRM
read x <&3
echo "<$x>"
stop_nudge
trap - ALRM
# The action sees what the read assigned, and its status in $?.
x=old seen=unset st=unset
trap 'seen=$x st=$?' USR1
printf 'partial' >&3
nudge USR1
read x <&3
echo "seen=<$seen> status=$st x=<$x>"
stop_nudge
