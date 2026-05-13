#!/bin/sh
# POSIX_REF: 2.14.14 dot
# DESCRIPTION: dot searches PATH when argument has no slash
# EXPECT_OUTPUT: found
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/libdir"
cat > "$TEST_TMPDIR/libdir/mylib.sh" <<'EOF'
echo found
EOF
PATH="$TEST_TMPDIR/libdir:$PATH"
export PATH
. mylib.sh
