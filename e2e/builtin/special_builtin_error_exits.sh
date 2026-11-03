#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A special built-in utility error exits a non-interactive shell (each case runs in a subshell)
# EXPECT_OUTPUT<<END
# set rc=2
# unset rc=2
# shift rc=2
# dot rc=1
# export rc=1
# readonly rc=1
# command set survived
# eval set rc=2
# END
( set -Z; echo survived ) 2>/dev/null; echo "set rc=$?"
( unset -Z x; echo survived ) 2>/dev/null; echo "unset rc=$?"
( set -- a; shift abc; echo survived ) 2>/dev/null; echo "shift rc=$?"
( . ./nonexistent_yosh_file; echo survived ) 2>/dev/null; echo "dot rc=$?"
( readonly r=1; export r=2; echo survived ) 2>/dev/null; echo "export rc=$?"
( readonly r=1; readonly r=2; echo survived ) 2>/dev/null; echo "readonly rc=$?"
( command set -Z; echo command set survived ) 2>/dev/null
( eval 'set -Z'; echo survived ) 2>/dev/null; echo "eval set rc=$?"
