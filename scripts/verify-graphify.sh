#!/usr/bin/env bash
# Checks that Graphify (developer-navigation only, see AGENTS.md) is set up
# correctly: pinned version, graph exists, secrets excluded, MCP tools listed.
set -euo pipefail
cd "$(dirname "$0")/.."

PINNED_VERSION="0.9.48"
fail=0

check() {
  if [ "$2" = "ok" ]; then
    echo "OK   $1"
  else
    echo "FAIL $1"
    fail=1
  fi
}

version_output="$(graphify --version 2>&1 || true)"
if echo "$version_output" | grep -q "$PINNED_VERSION"; then
  check "graphify version pinned to $PINNED_VERSION" ok
else
  check "graphify version pinned to $PINNED_VERSION (got: $version_output)" fail
fi

command -v graphify-mcp >/dev/null 2>&1 && check "graphify-mcp on PATH" ok || check "graphify-mcp on PATH" fail

if [ -f graphify-out/graph.json ]; then
  check "graphify-out/graph.json exists" ok
else
  check "graphify-out/graph.json exists" fail
fi

if grep -q "^graphify-out/" .gitignore 2>/dev/null; then
  check "graphify-out/ excluded in .gitignore" ok
else
  check "graphify-out/ excluded in .gitignore" fail
fi

if [ -f .graphifyignore ] && grep -q "\.env" .graphifyignore && grep -q "\*\.pem" .graphifyignore && grep -q "\*\.key" .graphifyignore; then
  check ".graphifyignore excludes .env*/*.pem/*.key" ok
else
  check ".graphifyignore excludes .env*/*.pem/*.key" fail
fi

expected_tools=(query_graph get_node get_neighbors get_community god_nodes graph_stats shortest_path list_prs get_pr_impact triage_prs graphify)
missing=0
for t in "${expected_tools[@]}"; do
  grep -q "\"$t\"" .mcp.json 2>/dev/null || missing=1
done
if [ "$missing" -eq 0 ]; then
  check "all 11 read-only MCP tools allow-listed in .mcp.json" ok
else
  check "all 11 read-only MCP tools allow-listed in .mcp.json" fail
fi

exit $fail
