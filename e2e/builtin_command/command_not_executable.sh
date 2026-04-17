#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command returns 126 when PATH target is a regular file without execute bit
# EXPECT_EXIT: 126
# EXPECT_STDERR: permission denied
mkdir -p /tmp/yosh_e2e_noexec
printf '#!/bin/sh\necho hi\n' > /tmp/yosh_e2e_noexec/cmd
chmod 644 /tmp/yosh_e2e_noexec/cmd
PATH=/tmp/yosh_e2e_noexec command cmd
