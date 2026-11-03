#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: under set -a a temporary prefix assignment is undone verbatim, so an unexported variable stays unexported (dash agrees)
# EXPECT_OUTPUT<<END
# builtin: unset old
# function: unset old
# was unset: unset unset
# special: new
# END
X=old
set -a
X=new true
set +a
/bin/sh -c 'echo "builtin: ${X-unset} $0"' "$X"
f() { :; }
set -a
X=new f
set +a
/bin/sh -c 'echo "function: ${X-unset} $0"' "$X"
unset X
set -a
X=new true
set +a
/bin/sh -c 'echo "was unset: ${X-unset} $0"' "${X-unset}"
X=old
set -a
X=new :
set +a
/bin/sh -c 'echo "special: ${X-unset}"'
