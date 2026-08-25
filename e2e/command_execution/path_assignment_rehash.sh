#!/bin/sh
# POSIX_REF: 2.9.5 hash / 2.5.3 PATH
# DESCRIPTION: Plain PATH assignment invalidates remembered utility locations
# EXPECT_OUTPUT<<END
# FROM_A
# FROM_B
# END
# EXPECT_EXIT: 0
dir=$(mktemp -d)
mkdir -p "$dir/a" "$dir/b"
printf '#!/bin/sh\necho FROM_A\n' > "$dir/a/yosh_rehash_tool"
printf '#!/bin/sh\necho FROM_B\n' > "$dir/b/yosh_rehash_tool"
chmod 755 "$dir/a/yosh_rehash_tool" "$dir/b/yosh_rehash_tool"
old_path=$PATH
PATH=$dir/a
yosh_rehash_tool
PATH=$dir/b
yosh_rehash_tool
PATH=$old_path
rm -rf "$dir"
