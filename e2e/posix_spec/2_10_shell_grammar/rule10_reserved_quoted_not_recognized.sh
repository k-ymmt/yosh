#!/bin/sh
# POSIX_REF: 2.10.2 Rule 10 - Keyword recognition
# DESCRIPTION: A quoted reserved word in command position is looked up as a command, not recognized as a keyword
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
# If quoted 'if' were still recognized as the reserved word, `'if' true`
# would start an incomplete if-statement and yield a syntax error (exit 2).
# To rule out the false-pass where an 'if' executable happens to exist on
# PATH and returns 0, we force command lookup to miss via PATH=/nonexistent
# and assert the outcome is exactly 127 (command not found).
PATH=/nonexistent/path 'if' true 2>/dev/null
rc=$?
if [ "$rc" -eq 2 ]; then
    echo "syntax error detected (rc=2); quoted 'if' was treated as reserved word" >&2
    exit 1
fi
if [ "$rc" -ne 127 ]; then
    echo "expected 127 (command not found), got $rc" >&2
    exit 1
fi
echo ok
