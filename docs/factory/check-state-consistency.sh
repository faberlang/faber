#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
index="$root/docs/factory/README.md"

check_goal_status() {
    goal_name=$1
    goal_path=$2
    goal_status=$(sed -n 's/^\*\*Status\*\*: //p' "$goal_path" | tr '\n' ' ' | sed 's/[[:space:]]*$//')
    index_status=$(sed -n "s/^| $goal_name | \\([^|]*\\) | .*$/\\1/p" "$index")

    if [ -z "$goal_status" ]; then
        echo "missing goal Status line: $goal_path" >&2
        exit 1
    fi

    if [ -z "$index_status" ]; then
        echo "missing factory index row for $goal_name: $index" >&2
        exit 1
    fi

    case "$goal_status" in
        "$index_status"*) ;;
        *)
            echo "$goal_name factory status mismatch" >&2
            echo "goal:  $goal_status" >&2
            echo "index: $index_status" >&2
            exit 1
            ;;
    esac
}

check_goal_status \
    "Autograd-equivalent roadmap" \
    "$root/docs/factory/autograd-equivalent-roadmap/deck.md"
check_goal_status \
    "Inference session boundary" \
    "$root/docs/factory/inference-session-boundary/goal.md"
check_goal_status \
    "SQLite library package" \
    "$root/docs/factory/sqlite-library-package/goal.md"

python3 "$root/docs/factory/autograd-equivalent-roadmap/check-autograd-roadmap.py" \
    "$root/docs/factory/autograd-equivalent-roadmap/deck.md"
python3 "$root/docs/factory/inference-session-boundary/check-model-artifact-oracle.py" \
    "$root/docs/factory/inference-session-boundary/model-artifact-oracle.toml"
python3 "$root/docs/factory/inference-session-boundary/check-session-cli-contract.py" \
    "$root/docs/factory/inference-session-boundary/session-cli-contract.toml"
python3 "$root/docs/factory/inference-session-boundary/check-session-cli-contract-negative.py"
