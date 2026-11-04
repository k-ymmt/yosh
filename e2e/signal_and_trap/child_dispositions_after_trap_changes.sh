#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: Every kind of child (fork+exec with a redirection, command substitution, pipeline member, subshell) sees trap-ignored signals ignored and every other signal the shell touched at its default, including traps that were set and then removed
# EXPECT_OUTPUT<<END
# INT=DFL QUIT=DFL TERM=DFL HUP=DFL USR1=IGNORE USR2=DFL PIPE=DFL ALRM=DFL
# INT=DFL QUIT=DFL TERM=DFL HUP=DFL USR1=IGNORE USR2=DFL PIPE=DFL ALRM=DFL
# INT=DFL QUIT=DFL TERM=DFL HUP=DFL USR1=IGNORE USR2=DFL PIPE=DFL ALRM=DFL
# INT=DFL QUIT=DFL TERM=DFL HUP=DFL USR1=DFL USR2=DFL PIPE=DFL ALRM=DFL
# INT=DFL QUIT=DFL TERM=DFL HUP=DFL USR1=DFL USR2=DFL PIPE=DFL ALRM=DFL
# INT=DFL QUIT=DFL TERM=IGNORE HUP=DFL USR1=DFL USR2=DFL PIPE=IGNORE ALRM=DFL
# END
# EXPECT_EXIT: 0
probe='for (qw(INT QUIT TERM HUP USR1 USR2 PIPE ALRM)) { push @o, "$_=" . ($SIG{$_} // "DFL") } print "@o\n"'
trap 'echo caught' USR2 ALRM
trap '' USR1
perl -e "$probe" 2>/dev/null
x=$(perl -e "$probe"); echo "$x"
perl -e "$probe" | cat
trap - USR1
trap '' PIPE
trap - PIPE
(perl -e "$probe"; :)
(trap '' TERM; (trap - TERM; perl -e "$probe"; :); :)
(trap '' TERM PIPE; perl -e "$probe"; :)
