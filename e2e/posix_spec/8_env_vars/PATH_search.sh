#!/bin/sh
# POSIX_REF: 8 Environment Variables - PATH
# DESCRIPTION: PATH is searched for external commands in order
# EXPECT_OUTPUT: from-dir1
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/d1" "$TEST_TMPDIR/d2"
cat > "$TEST_TMPDIR/d1/mycmd" <<'EOF'
#!/bin/sh
echo from-dir1
EOF
cat > "$TEST_TMPDIR/d2/mycmd" <<'EOF'
#!/bin/sh
echo from-dir2
EOF
chmod +x "$TEST_TMPDIR/d1/mycmd" "$TEST_TMPDIR/d2/mycmd"
PATH="$TEST_TMPDIR/d1:$TEST_TMPDIR/d2"
mycmd
