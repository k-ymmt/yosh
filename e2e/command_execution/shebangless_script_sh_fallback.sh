#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands (Command Search and Execution, item 1.e.i.b)
# DESCRIPTION: An executable file without a #! line is run via /bin/sh (execvp ENOEXEC fallback), for both PATH lookup and direct-path invocation
# EXPECT_OUTPUT<<END
# hello arg1
# hello arg2
# END
# EXPECT_EXIT: 0
printf 'echo hello "$1"\n' > "$TEST_TMPDIR/noshebang_cmd"
chmod 755 "$TEST_TMPDIR/noshebang_cmd"
PATH="$TEST_TMPDIR:$PATH"
noshebang_cmd arg1
"$TEST_TMPDIR/noshebang_cmd" arg2
