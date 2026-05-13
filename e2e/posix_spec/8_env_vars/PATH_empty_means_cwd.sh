#!/bin/sh
# POSIX_REF: 8 Environment Variables - PATH
# DESCRIPTION: an empty PATH entry (leading colon, embedded ::, trailing colon) means the current directory
# EXPECT_OUTPUT: from-cwd
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/mycwdcmd" <<'EOF'
#!/bin/sh
echo from-cwd
EOF
chmod +x "$TEST_TMPDIR/mycwdcmd"
cd "$TEST_TMPDIR"
PATH=":/usr/bin:/bin"
mycwdcmd
