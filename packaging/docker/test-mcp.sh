#!/bin/bash
# Tests the AI connection of the viewer in its Docker image, as the SuperCloud uses it (route B of
# OpenAEC-Blueprint docs/supercloud-koppeling.md): with OPENAEC_MCP_GEHEIM the viewer serves MCP
# over HTTP on port 3001; the tools carry the prefix viewer3d_; viewer3d_open opens a file from the
# work folder (/openaec) and viewer3d_opslaan writes next to it; requests from a browser (Origin)
# or without the secret are refused, and so are paths outside the work folder.
#
#   packaging/docker/test-mcp.sh <image> <model in a format the viewer opens> <output folder>
#
# Writes the answers, a screenshot of the 3D view (viewport.png) and container.log to the output
# folder. Needs Docker, curl and jq, as on the GitHub ubuntu runners.
set -euo pipefail

image=$1
model=$2
out=$3
port=${MCP_PORT:-3001}
mkdir -p "$out"
files=$(mktemp -d)
cp "$model" "$files/"
chmod 777 "$files"
chmod 666 "$files"/*
name="o3v-mcp-test-$$"
secret=$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')
file=$(basename "$model")
stem=${file%.*}

finish() {
  docker logs "$name" >"$out/container.log" 2>&1 || true
  docker rm -f "$name" >/dev/null 2>&1 || true
}
trap finish EXIT
fail() { echo "::error::$*"; exit 1; }

# The MCP port is published here for the test only; in the SuperCloud it stays internal.
docker run -d --name "$name" -p "127.0.0.1:$port:3001" -v "$files:/openaec" -e OPENAEC_MCP_GEHEIM="$secret" "$image" >/dev/null

# post NAME BODY [curl options]: the HTTP status; the body goes to NAME.json
post() {
  local what=$1 body=$2; shift 2
  curl -sS -o "$out/$what.json" -w '%{http_code}' -X POST "http://127.0.0.1:$port/mcp" \
    -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' "$@" --data "$body" || echo 000
}
auth=(-H "Authorization: Bearer $secret")
# tool NAME ARGS: calls a tool, its result in NAME.json
tool() {
  local code
  code=$(post "$1" "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"$1\",\"arguments\":$2}}" "${auth[@]}")
  [ "$code" = 200 ] || fail "$1 answered HTTP $code"
}

echo "Waiting until the MCP server answers"
for _ in $(seq 60); do
  [ "$(docker inspect -f '{{.State.Running}}' "$name")" = true ] || fail "the container stopped"
  code=$(post initialize '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}' "${auth[@]}")
  [ "$code" = 200 ] && break
  sleep 2
done
[ "$code" = 200 ] || fail "the MCP server did not answer (HTTP $code)"
jq -e '.result.protocolVersion == "2025-06-18"' "$out/initialize.json" >/dev/null || fail "initialize: $(cat "$out/initialize.json")"

echo "Refuses browsers, missing and wrong secrets"
[ "$(post origin '{}' "${auth[@]}" -H 'Origin: http://localhost:3000')" = 403 ] || fail "a request with an Origin header was not refused"
[ "$(post nosecret '{}')" = 401 ] || fail "a request without the secret was not refused"
[ "$(post wrongsecret '{}' -H 'Authorization: Bearer wrong-secret-wrong-secret')" = 401 ] || fail "a wrong secret was not refused"

echo "Lists the tools with the prefix viewer3d_"
[ "$(post tools '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' "${auth[@]}")" = 200 ] || fail "tools/list failed"
for t in viewer3d_open viewer3d_opslaan viewer3d_stand viewer3d_screenshot; do
  jq -e --arg t "$t" '[.result.tools[].name] | index($t) != null' "$out/tools.json" >/dev/null || fail "$t is not listed"
done
jq -e '[.result.tools[].name] | all(startswith("viewer3d_"))' "$out/tools.json" >/dev/null || fail "a tool has no prefix"
echo "$(jq '.result.tools | length' "$out/tools.json") tools"

echo "Opens $file from the work folder"
tool viewer3d_open "{\"path\":\"$file\"}"
jq -e '.result.isError == false' "$out/viewer3d_open.json" >/dev/null || fail "viewer3d_open: $(cat "$out/viewer3d_open.json")"
tool viewer3d_stand '{}'
jq -e --arg f "$file" '.result.content[0].text | contains($f)' "$out/viewer3d_stand.json" >/dev/null || fail "viewer3d_stand does not name $file"

echo "Refuses a path outside the work folder"
code=$(post outside '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"viewer3d_open","arguments":{"path":"/etc/passwd"}}}' "${auth[@]}")
[ "$code" = 200 ] || fail "viewer3d_open outside the work folder answered HTTP $code"
jq -e '.result.isError == true and (.result.content[0].text | contains("work folder"))' "$out/outside.json" >/dev/null || fail "a path outside the work folder was opened: $(cat "$out/outside.json")"

echo "Saves next to it as IFC"
tool viewer3d_opslaan '{}'
jq -e '.result.isError == false' "$out/viewer3d_opslaan.json" >/dev/null || fail "viewer3d_opslaan: $(cat "$out/viewer3d_opslaan.json")"
[ -s "$files/$stem.ifc" ] || fail "$stem.ifc was not written in the work folder"
head -c 64 "$files/$stem.ifc" | grep -q 'ISO-10303-21' || fail "$stem.ifc is not an IFC file"
echo "wrote $stem.ifc, $(wc -c <"$files/$stem.ifc") bytes"

echo "Takes a screenshot of the 3D view"
tool viewer3d_screenshot '{"max_size":800}'
jq -r '.result.content[] | select(.type == "image") | .data' "$out/viewer3d_screenshot.json" | base64 -d >"$out/viewport.png"
[ -s "$out/viewport.png" ] || fail "viewer3d_screenshot gave no image"
echo "All MCP tests passed"
