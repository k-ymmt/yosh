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
# after the bytes written below until the signal arrives.
exec 3<>idle
trap : USR1
x=old
printf 'partial' >&3
(sleep 1; kill -USR1 $$) &
read x <&3
echo "$? <$x>"
wait
printf 'part\\ial' >&3
(sleep 1; kill -USR1 $$) &
read -r x <&3
echo "$? <$x>"
wait
# The usual splitting applies to the partial line.
x=old y=old z=old
printf ' a  b c d ' >&3
(sleep 1; kill -USR1 $$) &
read x y z <&3
echo "$? <$x> <$y> <$z>"
wait
# A pending backslash escape is dropped, as at end-of-file.
printf 'part\\' >&3
(sleep 1; kill -USR1 $$) &
read x <&3
echo "$? <$x>"
wait
# Nothing read yet: the variable is set to the empty string.
x=old
(sleep 1; kill -USR1 $$) &
read x <&3
echo "$? <$x>"
wait
