#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
goal="$root/docs/factory/sqlite-library-package/goal.md"
index="$root/docs/factory/README.md"

goal_status=$(sed -n 's/^\*\*Status\*\*: //p' "$goal" | tr '\n' ' ' | sed 's/[[:space:]]*$//')
index_status=$(sed -n 's/^| SQLite library package | \([^|]*\) | .*$/\1/p' "$index")

if [ -z "$goal_status" ]; then
    echo "missing SQLite goal Status line: $goal" >&2
    exit 1
fi

if [ -z "$index_status" ]; then
    echo "missing SQLite library package row: $index" >&2
    exit 1
fi

case "$goal_status" in
    "$index_status"*) ;;
    *)
        echo "SQLite factory status mismatch" >&2
        echo "goal:  $goal_status" >&2
        echo "index: $index_status" >&2
        exit 1
        ;;
esac
