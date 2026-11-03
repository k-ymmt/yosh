#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: A temporary prefix assignment to an exported-but-unset variable restores the export attribute afterwards
# EXPECT_OUTPUT<<END
# unset
# final
# END
# EXPECT_EXIT: 0
unset x
export x
x=temp true
echo "${x-unset}"
x=final
/usr/bin/printenv x
