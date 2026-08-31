#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

assert_contains() {
  local file="$1"
  local expected="$2"
  if ! grep -Fq "$expected" "$file"; then
    echo "expected to find: $expected"
    echo "--- output ---"
    cat "$file"
    exit 1
  fi
}

assert_not_contains() {
  local file="$1"
  local unexpected="$2"
  if grep -Fq "$unexpected" "$file"; then
    echo "did not expect to find: $unexpected"
    echo "--- output ---"
    cat "$file"
    exit 1
  fi
}

run_case() {
  local scenario="$1"
  local expected_exit="$2"
  local output_file="${TMP_DIR}/${scenario}.log"
  local mock_bin_dir="${TMP_DIR}/${scenario}-bin"

  mkdir -p "${mock_bin_dir}"

  cat > "${mock_bin_dir}/cargo" <<'EOF'
#!/usr/bin/env bash
if [[ "${1:-}" == "--version" ]]; then
  echo "cargo 1.81.0"
fi
exit 0
EOF
  chmod +x "${mock_bin_dir}/cargo"

  cat > "${mock_bin_dir}/rustc" <<'EOF'
#!/usr/bin/env bash
if [[ "${1:-}" == "--version" ]]; then
  echo "rustc 1.81.0"
fi
exit 0
EOF
  chmod +x "${mock_bin_dir}/rustc"

  cat > "${mock_bin_dir}/soroban" <<EOF
#!/usr/bin/env bash
set -euo pipefail
scenario="${scenario}"
command="\$1"
shift

if [[ "\${command}" == "--version" ]]; then
  echo "soroban 21.0.0"
  exit 0
fi

if [[ "\${command}" == "keys" && "\${1:-}" == "address" ]]; then
  echo "GADMINADDRESS"
  exit 0
fi

if [[ "\${command}" == "contract" && "\${1:-}" == "optimize" ]]; then
  exit 0
fi

if [[ "\${command}" == "contract" && "\${1:-}" == "deploy" ]]; then
  case "\${scenario}" in
    deploy-rpc-unavailable)
      echo "error: request failed: connection refused" >&2
      exit 1
      ;;
    *)
      echo "CID1234567890"
      exit 0
      ;;
  esac
fi

if [[ "\${command}" == "contract" && "\${1:-}" == "invoke" ]]; then
  action=""
  for arg in "\$@"; do
    if [[ "\${arg}" == "initialize" || "\${arg}" == "get_admin" || "\${arg}" == "get_version" ]]; then
      action="\${arg}"
    fi
  done

  case "\${scenario}:\${action}" in
    initialize-timeout:initialize)
      echo "error: transaction submission timed out waiting for confirmation" >&2
      exit 1
      ;;
    initialize-bad-sequence:initialize)
      echo "tx_bad_seq" >&2
      exit 1
      ;;
    verify-rpc-unavailable:get_admin)
      echo "error: http request failed: dns error" >&2
      exit 1
      ;;
    success:get_admin)
      echo "GADMINADDRESS"
      exit 0
      ;;
    success:get_version)
      echo "1"
      exit 0
      ;;
    initialize-timeout:get_version|initialize-bad-sequence:get_version)
      echo "should not be called" >&2
      exit 1
      ;;
    *:initialize)
      exit 0
      ;;
    *:get_admin)
      echo "GADMINADDRESS"
      exit 0
      ;;
    *:get_version)
      echo "1"
      exit 0
      ;;
  esac
fi

echo "unexpected mock soroban invocation: \$command \$*" >&2
exit 1
EOF
  chmod +x "${mock_bin_dir}/soroban"

  set +e
  PATH="${mock_bin_dir}:${PATH}" \
    "${ROOT_DIR}/deploy.sh" testnet deployer GSERVICEADDRESS >"${output_file}" 2>&1
  local exit_code=$?
  set -e

  if [[ "${exit_code}" -ne "${expected_exit}" ]]; then
    echo "scenario ${scenario} exited ${exit_code}, expected ${expected_exit}"
    cat "${output_file}"
    exit 1
  fi

  LAST_OUTPUT_FILE="${output_file}"
}

run_case success 0
assert_contains "${LAST_OUTPUT_FILE}" "Deployment complete"
assert_contains "${LAST_OUTPUT_FILE}" "Contract:   CID1234567890"

run_case deploy-rpc-unavailable 1
assert_contains "${LAST_OUTPUT_FILE}" "RPC endpoint appears unavailable"
assert_not_contains "${LAST_OUTPUT_FILE}" "Deployment complete"

run_case initialize-timeout 1
assert_contains "${LAST_OUTPUT_FILE}" "deployment state is unconfirmed"
assert_contains "${LAST_OUTPUT_FILE}" "Contract id: CID1234567890"
assert_not_contains "${LAST_OUTPUT_FILE}" "Deployment complete"

run_case initialize-bad-sequence 1
assert_contains "${LAST_OUTPUT_FILE}" "Sequence number rejected"
assert_not_contains "${LAST_OUTPUT_FILE}" "Deployment complete"

run_case verify-rpc-unavailable 1
assert_contains "${LAST_OUTPUT_FILE}" "Post-deployment verification failed"
assert_contains "${LAST_OUTPUT_FILE}" "Contract id: CID1234567890"
assert_not_contains "${LAST_OUTPUT_FILE}" "Deployment complete"

echo "deploy script failure-mode tests passed"
