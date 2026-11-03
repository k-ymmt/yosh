#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: Redirection failures report the system error message, not a symbolic errno name
# EXPECT_OUTPUT<<END
# yosh: /nonexistent_dir_zz/x: No such file or directory
# yosh: 7: Bad file descriptor
# END
{ echo >/nonexistent_dir_zz/x; } 2>&1
exec 7>&-
{ echo x >&7; } 2>&1 || :
