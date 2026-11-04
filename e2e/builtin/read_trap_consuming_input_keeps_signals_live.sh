#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: A trap action that consumes read's pending input does not leave read blocked and deaf to later trapped signals
# EXPECT_OUTPUT<<END
# INNER:abc
# status=1 x=<> alarm=yes
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
mkfifo in || exit 1
exec 3<>in
trap 'read y; echo "INNER:$y"' USR1
trap 'alarm=yes' ALRM
# While the shell is stopped inside read, input arrives and USR1 becomes
# pending, so read resumes with data ready and runs the USR1 trap, which
# reads that very input. read must then wait for input again (not block
# in read(2)), so ALRM ends it long before more input arrives.
(
  sleep 1
  kill -STOP $$
  printf 'abc\n' >&3
  kill -USR1 $$
  kill -CONT $$
  sleep 2
  kill -ALRM $$
  sleep 2
  printf 'zzz\n' >&3
) &
read x <&3
echo "status=$? x=<$x> alarm=${alarm-no}"
wait
