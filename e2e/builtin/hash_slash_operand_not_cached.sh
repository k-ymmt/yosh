#!/bin/sh
# POSIX_REF: 4 Utilities - hash
# DESCRIPTION: hash ignores operands containing a slash instead of caching them under their basename
# EXPECT_OUTPUT<<END
# 0
# real-ls
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
mkdir d2 bin
printf '#!/bin/sh\necho fake\n' > d2/mycmd
printf '#!/bin/sh\necho real-ls\n' > bin/mycmd
chmod +x d2/mycmd bin/mycmd
PATH="$TEST_TMPDIR/bin:$PATH"
hash d2/mycmd; echo $?
mycmd
