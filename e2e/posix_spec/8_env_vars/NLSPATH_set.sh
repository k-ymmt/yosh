#!/bin/sh
# POSIX_REF: 8 Environment Variables - NLSPATH
# DESCRIPTION: NLSPATH locates message catalogs; yosh does not use catgets
# EXPECT_EXIT: 0
NLSPATH=/usr/share/locale/%L/LC_MESSAGES/%N.cat
exit 0
