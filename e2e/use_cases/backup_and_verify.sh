#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Use case - back up a file, verify the copy, then restore it
# EXPECT_OUTPUT<<END
# backup identical
# restored
# END
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
printf '%s\n' "important data" > config.ini
cp config.ini config.ini.bak
if cmp -s config.ini config.ini.bak; then
  echo "backup identical"
fi
echo "corrupted" > config.ini
mv config.ini.bak config.ini
if [ "$(cat config.ini)" = "important data" ]; then
  echo "restored"
fi
