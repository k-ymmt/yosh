#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A fatal assignment error in an if/elif/while/until condition, a case item, or a called function ends the shell without running any other branch
# EXPECT_OUTPUT<<END
# if: exited
# elif: exited
# while: exited
# until: exited
# case-fallthrough: exited
# function-condition: exited
# errexit-if: exited
# END
# EXPECT_EXIT: 0
exec 2>/dev/null
readonly r=1
out=$(if r=2 /bin/sh -c :; then echo THEN; else echo ELSE; fi; echo after)
[ $? -ne 0 ] && [ -z "$out" ] && echo "if: exited"
out=$(if false; then :; elif r=2 /bin/sh -c :; then echo THEN; else echo ELSE; fi; echo after)
[ $? -ne 0 ] && [ -z "$out" ] && echo "elif: exited"
out=$(while r=2 /bin/sh -c :; do echo BODY; break; done; echo after)
[ $? -ne 0 ] && [ -z "$out" ] && echo "while: exited"
out=$(until r=2 /bin/sh -c :; do echo BODY; break; done; echo after)
[ $? -ne 0 ] && [ -z "$out" ] && echo "until: exited"
out=$(case a in a) r=2 /bin/sh -c : ;& b) echo FALL;; esac; echo after)
[ $? -ne 0 ] && [ -z "$out" ] && echo "case-fallthrough: exited"
out=$(f() { r=2 /bin/sh -c :; }; if f; then echo THEN; else echo ELSE; fi; echo after)
[ $? -ne 0 ] && [ -z "$out" ] && echo "function-condition: exited"
out=$(set -e; if r=2 /bin/sh -c :; then echo THEN; else echo ELSE; fi; echo after)
[ $? -ne 0 ] && [ -z "$out" ] && echo "errexit-if: exited"
