#!/bin/sh
# POSIX_REF: 2.9.1.4 Command Search and Execution
# DESCRIPTION: A function shadows a PATH utility of the same name
# EXPECT_OUTPUT<<END
# real
# func
# END
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/shadowme" <<'SCRIPT'
#!/bin/sh
echo real
SCRIPT
chmod +x "$TEST_TMPDIR/shadowme"
PATH="$TEST_TMPDIR:$PATH"
shadowme
shadowme() { echo func; }
shadowme
