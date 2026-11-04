#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -v and type report a name containing a slash as given when it is executable, without a PATH search
# EXPECT_OUTPUT<<END
# ./tool 0
# sub/tool 0
# ./tool is ./tool
# 1
# 1
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
mkdir sub
printf '#!/bin/sh\n' > tool
chmod +x tool
cp tool sub/tool
: > plain
PATH=/nonexistent
echo "$(command -v ./tool) $?"
echo "$(command -v sub/tool) $?"
type ./tool
command -v ./plain; echo $?
command -v ./sub 2>/dev/null; echo $?
