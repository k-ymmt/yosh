#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: A trapped signal arriving while read is blocked runs its action and ends the read with a failure status
# EXPECT_OUTPUT<<END
# trapped
# interrupted x=
# trapped
# interrupted x=
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
mkfifo idle || exit 1
exec 3<>idle
# USR1 is caught with SA_RESTART, ALRM likewise: neither may be slept through.
trap 'echo trapped' USR1 ALRM
(sleep 1; kill -USR1 $$) &
read x <&3
[ $? -ne 0 ] && echo "interrupted x=$x"
(sleep 1; kill -ALRM $$) &
read x <&3
[ $? -ne 0 ] && echo "interrupted x=$x"
