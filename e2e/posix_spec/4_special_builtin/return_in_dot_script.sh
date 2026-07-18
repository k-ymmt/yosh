#!/bin/sh
# POSIX_REF: 2.15 return
# DESCRIPTION: return inside a dot-sourced script returns from the dot, not the parent shell
# EXPECT_OUTPUT<<END
# inside
# after-dot
# END
# EXPECT_EXIT: 0
cat > "$TEST_TMPDIR/sub.sh" <<'EOF'
echo inside
return 0
echo unreached
EOF
. "$TEST_TMPDIR/sub.sh"
echo after-dot
