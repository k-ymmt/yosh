#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: An earlier prefix assignment of an external command is exported to utilities run by a later one's command substitution, then fully rolled back
# EXPECT_OUTPUT<<END
# hello
# [unset]
# new
# old
# new
# [unexported]
# old
# new
# [unexported]
# fake
# gone
# END
# EXPECT_EXIT: 0
unset x
x=hello y=$(sh -c 'echo "$x"') sh -c 'echo "$y"'
echo "[${x-unset}]"

# Already exported: the earlier value is visible, the old one comes back.
e=old; export e
e=new y=$(sh -c 'echo "$e"') sh -c 'echo "$y"'
sh -c 'echo "$e"'

# Not exported before: visible while expanding, unexported again afterwards.
u=old
u=new y=$(sh -c 'echo "${u-}"') sh -c 'echo "$y"'
sh -c 'echo "[${u-unexported}]"'
echo "$u"

# set -a does not leave the restored variable exported.
a=old
set -a
a=new y=$(sh -c 'echo "$a"') sh -c 'echo "$y"'
set +a
sh -c 'echo "[${a-unexported}]"'

# A PATH prefix assignment is used by a later one's command substitution,
# and the shell's own lookups forget it afterwards.
printf '#!/bin/sh\necho fake\n' > "$TEST_TMPDIR/r3cmd"
chmod 755 "$TEST_TMPDIR/r3cmd"
PATH="$TEST_TMPDIR:$PATH" y=$(r3cmd) sh -c 'echo "$y"'
r3cmd 2>/dev/null || echo gone
