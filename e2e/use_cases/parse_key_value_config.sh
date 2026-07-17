#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Use case - parse key=value config file, skipping comments and blank lines
# EXPECT_OUTPUT<<END
# host -> localhost
# port -> 8080
# END
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
cat > app.conf <<EOF
# application settings
host=localhost

port=8080
EOF
while IFS= read -r line; do
  case $line in
    ''|'#'*) continue ;;
  esac
  key=${line%%=*}
  value=${line#*=}
  echo "$key -> $value"
done < app.conf
