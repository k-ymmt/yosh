#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables - IFS
# DESCRIPTION: Use case - parse passwd-style colon-delimited record with IFS and read
# EXPECT_OUTPUT: user=alice uid=1001 home=/home/alice
line="alice:x:1001:1001:Alice Doe:/home/alice:/bin/sh"
IFS=: read -r user pass uid gid gecos home shell <<EOF
$line
EOF
echo "user=$user uid=$uid home=$home"
