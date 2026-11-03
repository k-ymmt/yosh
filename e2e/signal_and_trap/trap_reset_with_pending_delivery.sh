#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: A trap that resets itself does not let an already-pending delivery kill the shell; ignore-default signals never terminate it
# EXPECT_OUTPUT<<END
# caught WINCH
# caught TERM
# u
# survived
# END
# EXPECT_EXIT: 0
trap 'echo caught WINCH; trap - WINCH' WINCH
kill -s WINCH $$ $$
trap 'echo caught TERM; trap - TERM' TERM
kill -s TERM $$ $$
trap 'echo u; trap - WINCH' USR1
trap ':' WINCH
(kill -s USR1 $$; kill -s WINCH $$)
echo survived
