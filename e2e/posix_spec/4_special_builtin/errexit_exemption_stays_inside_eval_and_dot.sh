#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: The set -e exemption of an AND-OR list ending eval or dot text does not exempt the eval or dot command itself
# EXPECT_OUTPUT<<END
# eval 1
# alias 1
# syntax 2
# dot 1
# function 1
# brace ok
# or-list ok
# END
# EXPECT_EXIT: 0
(set -e; eval 'false && true'; echo leaked); echo "eval $?"
(set -e; eval 'alias fail="false && true"
fail'; echo leaked); echo "alias $?"
(set -e; eval 'false && :
)'; echo leaked) 2>/dev/null; echo "syntax $?"
echo 'false && true' > "$TEST_TMPDIR/s.sh"
(set -e; . "$TEST_TMPDIR/s.sh"; echo leaked); echo "dot $?"
(f() { false && true; }; set -e; f; echo leaked); echo "function $?"
# A compound command still propagates the exemption, and eval in a
# tested position is still exempt.
(set -e; { false && true; }; echo brace ok)
(set -e; eval 'false && true' || echo or-list ok)
