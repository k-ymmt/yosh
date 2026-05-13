#!/bin/sh
# POSIX_REF: 8 Environment Variables - IFS
# DESCRIPTION: IFS containing tab still splits on tab
# EXPECT_OUTPUT: 3
# EXPECT_EXIT: 0
IFS='	'  # literal tab
v='a	b	c'
set -- $v
echo $#
