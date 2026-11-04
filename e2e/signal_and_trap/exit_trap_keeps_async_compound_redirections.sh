#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: The redirections of a compound command run as an asynchronous list set up that subshell's environment, so its EXIT trap writes through them; redirections inside it are still unwound
# EXPECT_OUTPUT<<END
# status 7
# brace exit
# brace end
# if exit
# loop end
# fatal
# and-or
# inner unwound
# END
cd "$TEST_TMPDIR" || exit 1
exec 2>/dev/null
{ trap 'echo brace exit' EXIT; exit 7; } >f1 & wait $!
echo "status $?"
cat f1
{ trap 'echo brace end' EXIT; true; } >f1 & wait
cat f1
if :; then trap 'echo if exit' EXIT; exit; fi >f1 & wait
cat f1
for i in a; do trap 'echo loop end' EXIT; done >f1 & wait
cat f1
{ trap 'echo fatal' EXIT; : "${unset_var?}"; } >f1 & wait
cat f1
# Only a lone compound command is the subshell's body: in an AND-OR
# list its redirections are temporary (dash and bash).
{ trap 'echo and-or' EXIT; exit; } >/dev/null && : & wait
# Redirections inside the async payload are temporary too.
{ trap 'echo inner unwound' EXIT; { exit; } >/dev/null; } >f1 & wait
cat f1
