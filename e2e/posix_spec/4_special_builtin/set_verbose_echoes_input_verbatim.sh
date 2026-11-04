#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -v writes script and dot-script input to stderr exactly as read: comments, blank lines, indentation and raw bytes included
# EXPECT_OUTPUT<<END
# dot ok
# script ok
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
printf '# before\n  echo a\n\n# after\n' > d.sh
printf 'echo \377 # c\n' > raw.sh
{ set -v; . ./d.sh; . ./raw.sh; set +v; } 2>err >/dev/null
printf '# before\n  echo a\n\n# after\necho \377 # c\n' > want
if cmp -s err want; then echo dot ok; else od -c err; fi
printf 'set -v\n# c1\n  echo b  # c2\n\n# end\n' > top.sh
"$OLDPWD/target/debug/yosh" ./top.sh 2>err >/dev/null
printf '# c1\n  echo b  # c2\n\n# end\n' > want
if cmp -s err want; then echo script ok; else od -c err; fi
