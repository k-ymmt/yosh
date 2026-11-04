#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: exec keeps trap-ignored signals and the async-list SIGINT/SIGQUIT ignore, and resets caught ones
# EXPECT_OUTPUT<<END
# INT=IGNORE QUIT=IGNORE TERM=DFL TSTP=DFL
# INT=DFL QUIT=DFL TERM=IGNORE TSTP=DFL
# INT=DFL QUIT=DFL TERM=DFL TSTP=DFL
# END
# EXPECT_EXIT: 0
probe='for (qw(INT QUIT TERM TSTP)) { push @o, "$_=" . ($SIG{$_} // "DFL") } print "@o\n"'
{ exec perl -e "$probe"; } &
wait
(trap '' TERM; exec perl -e "$probe")
(trap 'echo caught' TERM; exec perl -e "$probe")
