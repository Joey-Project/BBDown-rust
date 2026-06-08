#!/usr/bin/env bash

release_list_matching_refs() {
  local repo=$1
  local prefix=$2
  local refs
  local error_file
  error_file=$(mktemp)
  if refs=$(gh api --paginate "repos/${repo}/git/matching-refs/tags/${prefix}" --jq '.[].ref' 2>"${error_file}"); then
    printf '%s\n' "${refs}"
    return
  fi
  if grep -q "HTTP 404" "${error_file}"; then
    return
  fi
  echo "::error::failed to list matching tags for ${prefix}: $(tr '\n' ' ' < "${error_file}")"
  exit 2
}

release_id_for_tag() {
  local repo=$1
  local tag=$2
  local ids
  local error_file
  error_file=$(mktemp)
  if ids=$(gh api --paginate "repos/${repo}/releases?per_page=100" --jq ".[] | select(.tag_name == \"${tag}\") | .id" 2>"${error_file}"); then
    printf '%s\n' "${ids}" | sed -n '1p'
    return
  fi
  echo "::error::failed to list GitHub Releases for ${tag}: $(tr '\n' ' ' < "${error_file}")"
  exit 2
}

release_ensure_release_absent() {
  local repo=$1
  local tag=$2
  local release_id
  release_id=$(release_id_for_tag "${repo}" "${tag}")
  if [[ -n "${release_id}" ]]; then
    echo "::error::GitHub Release ${tag} already exists; create a new version instead"
    exit 2
  fi
}

release_ensure_tag_absent() {
  local repo=$1
  local tag=$2
  local exists_message=$3
  local error_file
  error_file=$(mktemp)
  if gh api "repos/${repo}/git/ref/tags/${tag}" >/dev/null 2>"${error_file}"; then
    echo "::error::${exists_message}"
    exit 2
  fi
  if ! grep -q "HTTP 404" "${error_file}"; then
    echo "::error::failed to check tag ${tag}: $(tr '\n' ' ' < "${error_file}")"
    exit 2
  fi
}

release_max_rc_number() {
  local repo=$1
  local version=$2
  local rc_prefix="v${version}-rc."
  local version_regex="${version//./\\.}"
  local max_rc=0
  local refs
  local ref
  local tag
  local current_rc
  refs=$(release_list_matching_refs "${repo}" "${rc_prefix}")
  while IFS= read -r ref; do
    tag="${ref#refs/tags/}"
    if [[ "${tag}" =~ ^v${version_regex}-rc\.([1-9][0-9]*)$ ]]; then
      current_rc="${BASH_REMATCH[1]}"
      if (( current_rc > max_rc )); then
        max_rc="${current_rc}"
      fi
    fi
  done <<< "${refs}"
  printf '%s\n' "${max_rc}"
}

release_workspace_versions() {
  cargo metadata --no-deps --format-version 1 | python3 -c 'import json, sys; data = json.load(sys.stdin); versions = {pkg["name"]: pkg["version"] for pkg in data["packages"]}; print(versions["bbdown-core"]); print(versions["bbdown-cli"])'
}

release_tag_target_sha() {
  local repo=$1
  local tag=$2
  local tag_ref_json
  local tag_object_type
  local tag_object_sha
  tag_ref_json=$(gh api "repos/${repo}/git/ref/tags/${tag}")
  tag_object_type=$(printf '%s' "${tag_ref_json}" | python3 -c 'import json, sys; print(json.load(sys.stdin)["object"]["type"])')
  tag_object_sha=$(printf '%s' "${tag_ref_json}" | python3 -c 'import json, sys; print(json.load(sys.stdin)["object"]["sha"])')
  if [[ "${tag_object_type}" == "tag" ]]; then
    gh api "repos/${repo}/git/tags/${tag_object_sha}" --jq .object.sha
  else
    printf '%s\n' "${tag_object_sha}"
  fi
}
