#!/bin/sh
# POSIX_REF: 2.14 exec
# DESCRIPTION: exec PATH search finding a non-executable file reports permission denied, 126
# EXPECT_EXIT: 126
# EXPECT_STDERR: permission denied
d=$(mktemp -d)
printf '#!/bin/sh\necho hi\n' > "$d/noexec_target"
chmod 644 "$d/noexec_target"
# Absolute rm: the PATH prefix assignment below persists past the failed
# exec (special builtin), so the EXIT trap must not rely on PATH.
trap '/bin/rm -rf "$d"' EXIT
PATH=$d exec noexec_target
echo survived
