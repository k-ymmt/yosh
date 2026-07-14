#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: A quoted command word is not alias-substituted
# EXPECT_OUTPUT<<END
# aliased
# real
# real
# END
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/mycmd" <<'SCRIPT'
#!/bin/sh
echo real
SCRIPT
chmod +x "$TEST_TMPDIR/mycmd"
PATH="$TEST_TMPDIR:$PATH"
alias mycmd='echo aliased'
mycmd
\mycmd
'mycmd'
