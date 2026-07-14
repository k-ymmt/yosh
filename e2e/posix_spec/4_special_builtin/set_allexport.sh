#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -a exports subsequent assignments
# EXPECT_OUTPUT: v_allexport_kish=exported
# EXPECT_EXIT: 0
set -a
v_allexport_kish=exported
env | grep '^v_allexport_kish='
