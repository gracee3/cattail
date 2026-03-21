#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/cattail-smoke.XXXXXX")"
CATTAIL_PID=""

cleanup() {
  if [[ -n "${CATTAIL_PID}" ]] && kill -0 "${CATTAIL_PID}" 2>/dev/null; then
    kill "${CATTAIL_PID}" 2>/dev/null || true
    wait "${CATTAIL_PID}" 2>/dev/null || true
  fi
  rm -rf "${TMP_DIR}"
}
trap cleanup EXIT INT TERM

log() {
  printf '\n[%s] %s\n' "$(date +%H:%M:%S)" "$*" >&2
}

build_binary() {
  log "building cattail"
  cargo build --quiet --manifest-path "${ROOT_DIR}/Cargo.toml"
}

seed_file() {
  local path="$1"
  shift
  : > "${path}"
  for line in "$@"; do
    printf '%s\n' "${line}" >> "${path}"
  done
}

build_binary

file_a="${TMP_DIR}/orcas.log"
file_b="${TMP_DIR}/orcasd.log"
file_c="${TMP_DIR}/worker.log"

seed_file "${file_a}" \
  "orcas: booting" \
  "orcas: loading config" \
  "orcas: ready"
seed_file "${file_b}" \
  "orcasd: initializing" \
  "orcasd: listening"
seed_file "${file_c}" \
  "worker: start"

log "temp dir: ${TMP_DIR}"
log "phase 1: launch cattail with the seeded backlog"
log "phase 2: append new lines"
log "phase 3: truncate ${file_b}"
log "phase 4: recreate ${file_c}"

"${ROOT_DIR}/target/debug/cattail" \
  --interval-ms 100 \
  --prefix basename \
  "${file_a}" "${file_b}" "${file_c}" &
CATTAIL_PID=$!

sleep 1
printf '%s\n' "orcas: first live line" >> "${file_a}"
printf '%s\n' "orcasd: first live line" >> "${file_b}"
printf '%s\n' "worker: first live line" >> "${file_c}"

sleep 1
log "truncating ${file_b} in place"
: > "${file_b}"
printf '%s\n' "orcasd: after truncation" >> "${file_b}"

sleep 1
log "deleting and recreating ${file_c}"
rm -f "${file_c}"
sleep 1
seed_file "${file_c}" \
  "worker: recreated line 1" \
  "worker: recreated line 2"

sleep 2
log "stopping cattail"
kill "${CATTAIL_PID}" 2>/dev/null || true
wait "${CATTAIL_PID}" 2>/dev/null || true
CATTAIL_PID=""

log "done"
