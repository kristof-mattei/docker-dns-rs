#!/bin/bash

set -euo pipefail

# Configuration
ORG=""
USER=""
PACKAGE_NAME="package"
PER_PAGE=100
DRY_RUN=false
SKIP_CONFIRMATION=false
CLEANUP_PR_IMAGES=true
PR_IMAGE_AGE_DAYS=30

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --org)
            if [[ -n "$USER" ]]; then
                echo "Error: --org and --user are mutually exclusive" >&2
                exit 1
            fi
            ORG="$2"
            shift 2
            ;;
        --user)
            if [[ -n "$ORG" ]]; then
                echo "Error: --org and --user are mutually exclusive" >&2
                exit 1
            fi
            USER="$2"
            shift 2
            ;;
        --package)
            PACKAGE_NAME="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        --yes)
            SKIP_CONFIRMATION=true
            shift
            ;;
        --skip-pr-cleanup)
            CLEANUP_PR_IMAGES=false
            shift
            ;;
        --pr-age-days)
            PR_IMAGE_AGE_DAYS="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 [--org ORG | --user USER] [--package PACKAGE_NAME] [--dry-run] [--yes] [--skip-pr-cleanup] [--pr-age-days DAYS] [--help]"
            echo "  --org              GitHub organization name"
            echo "  --user             GitHub username"
            echo "  --package          Package name (default: $PACKAGE_NAME)"
            echo "  --dry-run          Show what would be deleted without actually deleting"
            echo "  --yes              Skip confirmation prompt"
            echo "  --skip-pr-cleanup  Skip cleanup of old PR images"
            echo "  --pr-age-days      Age in days for PR images to be considered old (default: $PR_IMAGE_AGE_DAYS)"
            echo "  --help             Show this help message"
            echo ""
            echo "Note: --org and --user are mutually exclusive. One must be specified."
            echo "Note: Images with the 'edge' or 'latest' tag will never be deleted."
            echo "Note: Platform-specific images referenced by protected multi-platform manifests will not be deleted."
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

# Validate that either --org or --user is specified
if [[ -z "$ORG" && -z "$USER" ]]; then
    echo "Error: Either --org or --user must be specified" >&2
    exit 1
fi

# Set the target and API path based on whether we're using org or user
if [[ -n "$ORG" ]]; then
    TARGET="$ORG"
    API_PATH="/orgs/$ORG"
else
    TARGET="$USER"
    API_PATH="/users/$USER"
fi

REGISTRY_BASE="ghcr.io/$TARGET/$PACKAGE_NAME"

# Check if gh CLI is installed and authenticated
if ! command -v gh &> /dev/null; then
    echo "Error: GitHub CLI (gh) is not installed" >&2
    exit 1
fi

if ! gh auth status &> /dev/null; then
    echo "Error: Not authenticated with GitHub CLI. Run 'gh auth login' first." >&2
    exit 1
fi

<<<<<<< HEAD
||||||| 1226d6d1
# Function to get versions for a specific page
=======
# Check for delete:packages scope
# auth_scopes=$(gh auth status 2>&1)
# if ! echo "$auth_scopes" | grep --quiet --extended-regexp "(delete:packages|write:packages)"; then
#     echo "Error: GitHub token lacks required scope. Need 'delete:packages' or 'write:packages'." >&2
#     echo "       Run 'gh auth refresh --scopes delete:packages' to add the scope." >&2
#     exit 1
# fi

>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
# Check if jq is installed
if ! command -v jq &> /dev/null; then
    echo "Error: jq is not installed" >&2
    exit 1
fi

# Check if crane or docker is available for manifest inspection
MANIFEST_TOOL=""
if command -v crane &> /dev/null; then
    MANIFEST_TOOL="crane"
elif command -v docker &> /dev/null; then
    MANIFEST_TOOL="docker"
else
    echo "Warning: Neither 'crane' nor 'docker' found. Cannot inspect multi-platform manifests." >&2
    echo "         Platform-specific images may be incorrectly deleted." >&2
    echo "         Install 'crane' (go-containerregistry) or 'docker' to fix this." >&2
fi

# ========== UTILITY FUNCTIONS ==========

# Get versions for a specific page
get_versions_page() {
    local page=$1
    gh api \
        --header "Accept: application/vnd.github+json" \
        --header "X-GitHub-Api-Version: 2022-11-28" \
        "$API_PATH/packages/container/$PACKAGE_NAME/versions?per_page=$PER_PAGE&page=$page"
}

# Delete a version
delete_version() {
    local version_id=$1
    local description="$2"

    if [[ "$DRY_RUN" == "true" ]]; then
        echo "[DRY RUN] Would delete version ID: $version_id ($description)"
        return 0
    fi

    echo "Deleting version ID: $version_id ($description)"
    if gh api \
        --method DELETE \
        --header "Accept: application/vnd.github+json" \
        --header "X-GitHub-Api-Version: 2022-11-28" \
<<<<<<< HEAD
        "$API_PATH/packages/container/$PACKAGE_NAME/versions/$version_id" 2>/dev/null; then
||||||| 1226d6d1
        if [[ $? -eq 0 ]]; then
            echo "Successfully deleted version ID: $version_id"
        else
            echo "Failed to delete version ID: $version_id"
        fi
=======
        "$API_PATH/packages/container/$PACKAGE_NAME/versions/$version_id" 2> /dev/null; then
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
        echo "Successfully deleted version ID: $version_id"
        return 0
    else
        echo "Failed to delete version ID: $version_id" >&2
        return 1
    fi
}

# Check if a date is older than specified days
is_older_than_days() {
    local date_str="$1"
    local days="$2"

    local date_epoch
<<<<<<< HEAD
    if ! date_epoch=$(date -d "$date_str" +%s 2>/dev/null); then
||||||| 1226d6d1
    # Convert date to epoch timestamp
    local date_epoch=$(date -d "$date_str" +%s 2> /dev/null)
    if [[ $? -ne 0 ]]; then
=======
    if ! date_epoch=$(date --date "$date_str" +%s 2> /dev/null); then
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
        echo "Warning: Could not parse date: $date_str" >&2
        return 1
    fi

    local cutoff_epoch=$(($(date +%s) - (days * 24 * 60 * 60)))
    [[ $date_epoch -lt $cutoff_epoch ]]
}

# Tag pattern checks
has_pr_latest_tag() {
    local tags_json="$1"
<<<<<<< HEAD
    echo "$tags_json" | jq -e '.[] | select(test("^pr-.*-latest$"))' > /dev/null 2>&1
||||||| 1226d6d1
    echo "$tags_json" | jq -e '.[] | select(test("^pr-.*-latest$"))' > /dev/null
=======
    echo "$tags_json" | jq --exit-status '.[] | select(test("^pr-.*-latest$"))' > /dev/null 2>&1
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
}

has_pr_sha_tag() {
    local tags_json="$1"
<<<<<<< HEAD
    echo "$tags_json" | jq -e '.[] | select(test("^pr-[a-f0-9]{40}-[a-f0-9]{40}(-[a-zA-Z0-9_-]+)?$"))' > /dev/null 2>&1
||||||| 1226d6d1
    # Match pr-{sha}-{sha} or pr-{sha}-{sha}-{architecture}
    # SHA pattern: 40 character hex string (git SHA-1)
    echo "$tags_json" | jq -e '.[] | select(test("^pr-[a-f0-9]{40}-[a-f0-9]{40}(-[a-zA-Z0-9_-]+)?$"))' > /dev/null
=======
    echo "$tags_json" | jq --exit-status '.[] | select(test("^pr-[a-f0-9]{40}-[a-f0-9]{40}(-[a-zA-Z0-9_-]+)?$"))' > /dev/null 2>&1
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
}

has_edge_tag() {
    local tags_json="$1"
<<<<<<< HEAD
    echo "$tags_json" | jq -e '.[] | select(. == "edge")' > /dev/null 2>&1
||||||| 1226d6d1
    echo "$tags_json" | jq -e '.[] | select(. == "edge")' > /dev/null
=======
    echo "$tags_json" | jq --exit-status '.[] | select(. == "edge")' > /dev/null 2>&1
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
}

has_latest_tag() {
    local tags_json="$1"
<<<<<<< HEAD
    echo "$tags_json" | jq -e '.[] | select(. == "latest")' > /dev/null 2>&1
||||||| 1226d6d1
    echo "$tags_json" | jq -e '.[] | select(. == "latest")' > /dev/null
=======
    echo "$tags_json" | jq --exit-status '.[] | select(. == "latest")' > /dev/null 2>&1
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
}

has_attestation_tag() {
    local tags_json="$1"
<<<<<<< HEAD
    echo "$tags_json" | jq -e '.[] | select(test("^sha256-[a-f0-9]{64}$"))' > /dev/null 2>&1
||||||| 1226d6d1
    echo "$tags_json" | jq -e '.[] | select(test("^sha256-[a-f0-9]{64}$"))' > /dev/null
=======
    echo "$tags_json" | jq --exit-status '.[] | select(test("^sha256-[a-f0-9]{64}$"))' > /dev/null 2>&1
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
}

extract_sha256_from_attestation() {
    local tags_json="$1"
    echo "$tags_json" | jq --raw-output '.[] | select(test("^sha256-[a-f0-9]{64}$")) | sub("^sha256-"; "")'
}

# Get the first available tag for manifest inspection
get_first_tag() {
    local tags_json="$1"
<<<<<<< HEAD
    echo "$tags_json" | jq -r '.[0] // empty'
||||||| 1226d6d1
# Check if gh CLI is installed and authenticated
if ! command -v gh &> /dev/null; then
    echo "Error: GitHub CLI (gh) is not installed"
    exit 1
fi
=======
    echo "$tags_json" | jq --raw-output '.[0] // empty'
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
}

# Fetch manifest and extract referenced digests (for multi-platform images)
# Returns newline-separated list of sha256 digests (without 'sha256:' prefix)
get_referenced_digests() {
    local image_ref="$1"

    if [[ -z "$MANIFEST_TOOL" ]]; then
        return 0
    fi

    local manifest=""
    if [[ "$MANIFEST_TOOL" == "crane" ]]; then
<<<<<<< HEAD
        manifest=$(crane manifest "$image_ref" 2>/dev/null) || return 0
    elif [[ "$MANIFEST_TOOL" == "docker" ]]; then
        manifest=$(docker manifest inspect "$image_ref" 2>/dev/null) || return 0
    fi

    if [[ -z "$manifest" ]]; then
        return 0
    fi

    # Check if this is a manifest list/index (multi-platform)
    local media_type
    media_type=$(echo "$manifest" | jq -r '.mediaType // .schemaVersion // empty')

    # Multi-platform manifest indicators:
    # - application/vnd.oci.image.index.v1+json
    # - application/vnd.docker.distribution.manifest.list.v2+json
    # - Has .manifests array
    if echo "$manifest" | jq -e '.manifests' > /dev/null 2>&1; then
        # Extract digests from manifests array
        echo "$manifest" | jq -r '.manifests[].digest // empty' | sed 's/^sha256://'
    fi
}

# ========== PHASE 1: COLLECT ALL VERSION DATA ==========

echo "Querying container versions for $TARGET, package $PACKAGE_NAME..."

# Associative arrays for version data
declare -A version_tags        # version_id -> tags JSON
declare -A version_digest      # version_id -> sha256 digest (without prefix)
declare -A version_created     # version_id -> created_at timestamp
declare -A digest_to_version   # sha256 digest -> version_id
||||||| 1226d6d1
# ========== FIRST PASS: Collect images to delete ==========
=======
        manifest=$(crane manifest "$image_ref" 2> /dev/null) || return 0
    elif [[ "$MANIFEST_TOOL" == "docker" ]]; then
        manifest=$(docker buildx imagetools inspect --raw "$image_ref" 2> /dev/null) || return 0
    fi

    if [[ -z "$manifest" ]]; then
        return 0
    fi

    # Check if this is a manifest list/index (multi-platform)
    local media_type
    media_type=$(echo "$manifest" | jq --raw-output '.mediaType // .schemaVersion // empty')

    # Multi-platform manifest indicators:
    # - application/vnd.oci.image.index.v1+json
    # - application/vnd.docker.distribution.manifest.list.v2+json
    # - Has .manifests array
    if echo "$manifest" | jq --exit-status '.manifests' > /dev/null 2>&1; then
        # Extract digests from manifests array
        echo "$manifest" | jq --raw-output '.manifests[].digest // empty' | sed 's/^sha256://'
    fi
}

# ========== PHASE 1: COLLECT ALL VERSION DATA ==========

echo "Querying container versions for $TARGET, package $PACKAGE_NAME..."

# Associative arrays for version data
declare -A version_tags      # version_id -> tags JSON
declare -A version_digest    # version_id -> sha256 digest (without prefix)
declare -A version_created   # version_id -> created_at timestamp
declare -A digest_to_version # sha256 digest -> version_id
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962

# Arrays for tracking
all_version_ids=()

page=1
while true; do
    echo "Fetching page $page..."

    response=$(get_versions_page $page)

    if [[ -z "$response" ]]; then
        break
    fi

<<<<<<< HEAD
    if echo "$response" | jq -e '.message' > /dev/null 2>&1; then
        echo "Error: $(echo "$response" | jq -r '.message')" >&2
        exit 1
    fi

    version_count=$(echo "$response" | jq '. | length')
    if [[ "$version_count" -eq 0 ]]; then
        break
||||||| 1226d6d1
    # Extract version data
    page_data=$(echo "$response" | jq --raw-output '.[]')

    # If no data on this page, we're done
    if [[ -z "$page_data" ]]; then
        break
=======
    if echo "$response" | jq --exit-status '.message' > /dev/null 2>&1; then
        echo "Error: $(echo "$response" | jq --raw-output '.message')" >&2
        exit 1
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
    fi

<<<<<<< HEAD
    while IFS= read -r version; do
        [[ -z "$version" ]] && continue

        version_id=$(echo "$version" | jq -r '.id')
        tags=$(echo "$version" | jq -c '.metadata.container.tags')
        created_at=$(echo "$version" | jq -r '.created_at')
        name=$(echo "$version" | jq -r '.name // empty')

        all_version_ids+=("$version_id")
        version_tags["$version_id"]="$tags"
        version_created["$version_id"]="$created_at"

        # Extract digest from name (format: sha256:xxx)
        if [[ "$name" =~ ^sha256:([a-f0-9]{64})$ ]]; then
            digest="${BASH_REMATCH[1]}"
            version_digest["$version_id"]="$digest"
            digest_to_version["$digest"]="$version_id"
        fi
    done <<< "$(echo "$response" | jq -c '.[]')"
||||||| 1226d6d1
    # Process each version individually
    while IFS= read -r version; do
        if [[ -n "$version" ]]; then
            version_id=$(echo "$version" | jq --raw-output '.id')
            tags=$(echo "$version" | jq '.metadata.container.tags')
            created_at=$(echo "$version" | jq --raw-output '.created_at')

            # Skip any version with "edge" or "latest" tag - they're protected
            if has_edge_tag "$tags" || has_latest_tag "$tags"; then
                ((protected_versions++))
                continue
            fi

            # Check for untagged versions
            if echo "$tags" | jq -e '. == []' > /dev/null; then
                untagged_versions+=("$version_id")
            fi

            # Check for old PR images if cleanup is enabled
            if [[ "$CLEANUP_PR_IMAGES" == "true" ]] && is_older_than_days "$created_at" "$PR_IMAGE_AGE_DAYS"; then
                tags_str=$(echo "$tags" | jq --raw-output '.[]' | tr '\n' ',' | sed 's/,$//')

                # Check for pr-*-latest pattern
                if has_pr_latest_tag "$tags"; then
                    old_pr_versions+=("$version_id")
                    old_pr_descriptions+=("old PR latest image: $created_at [$tags_str]")
                # Check for pr-sha-sha(-arch) pattern
                elif has_pr_sha_tag "$tags"; then
                    old_pr_sha_versions+=("$version_id")
                    old_pr_sha_descriptions+=("old PR SHA image: $created_at [$tags_str]")
                fi
            fi
        fi
    done <<< "$(echo "$response" | jq -c '.[]')"

    # Check if we got a full page (if less than PER_PAGE, we're done)
    version_count=$(echo "$response" | jq '. | length')
    if [[ $version_count -lt $PER_PAGE ]]; then
=======
    version_count=$(echo "$response" | jq '. | length')
    if [[ "$version_count" -eq 0 ]]; then
        break
    fi

    while IFS= read -r version; do
        [[ -z "$version" ]] && continue

        version_id=$(echo "$version" | jq --raw-output '.id')
        tags=$(echo "$version" | jq --compact-output '.metadata.container.tags')
        created_at=$(echo "$version" | jq --raw-output '.created_at')
        name=$(echo "$version" | jq --raw-output '.name // empty')

        all_version_ids+=("$version_id")
        version_tags["$version_id"]="$tags"
        version_created["$version_id"]="$created_at"

        # Extract digest from name (format: sha256:xxx)
        if [[ "$name" =~ ^sha256:([a-f0-9]{64})$ ]]; then
            digest="${BASH_REMATCH[1]}"
            version_digest["$version_id"]="$digest"
            digest_to_version["$digest"]="$version_id"
        fi
    done <<< "$(echo "$response" | jq --compact-output '.[]')"
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962

    if [[ "$version_count" -lt "$PER_PAGE" ]]; then
        break
    fi

    ((page++))
done

echo "Found ${#all_version_ids[@]} total versions"

# ========== PHASE 2: DETERMINE PROTECTED VERSIONS ==========

echo ""
echo "=== PHASE 2: DETERMINING PROTECTED VERSIONS ==="

# Protected digests: sha256 hashes that must not be deleted
<<<<<<< HEAD
declare -A protected_digests   # sha256 -> reason

# Track versions by category
declare -A protected_versions  # version_id -> reason
declare -A delete_candidates   # version_id -> reason

for version_id in "${all_version_ids[@]}"; do
    tags="${version_tags[$version_id]}"
    created_at="${version_created[$version_id]}"
    digest="${version_digest[$version_id]:-}"

    # Check for permanently protected tags
    if has_edge_tag "$tags" || has_latest_tag "$tags"; then
        protected_versions["$version_id"]="has edge/latest tag"

        if [[ -n "$digest" ]]; then
            protected_digests["$digest"]="referenced by edge/latest tagged image"
        fi

        # Fetch manifest to protect referenced platform-specific images
        first_tag=$(get_first_tag "$tags")
        if [[ -n "$first_tag" ]]; then
            echo "Inspecting manifest for protected image: $REGISTRY_BASE:$first_tag"
            while IFS= read -r ref_digest; do
                [[ -z "$ref_digest" ]] && continue
                protected_digests["$ref_digest"]="referenced by $first_tag manifest"
                echo "  Protected platform-specific digest: ${ref_digest:0:12}..."
            done <<< "$(get_referenced_digests "$REGISTRY_BASE:$first_tag")"
        fi
        continue
    fi

    # Check for attestations - handle separately later
    if has_attestation_tag "$tags"; then
        continue
    fi

    # Check for untagged versions
    if echo "$tags" | jq -e '. == []' > /dev/null 2>&1; then
        delete_candidates["$version_id"]="untagged"
        continue
    fi

    # Check for old PR images
    if [[ "$CLEANUP_PR_IMAGES" == "true" ]]; then
        if is_older_than_days "$created_at" "$PR_IMAGE_AGE_DAYS"; then
            tags_str=$(echo "$tags" | jq -r '.[]' | tr '\n' ',' | sed 's/,$//')

            if has_pr_latest_tag "$tags"; then
                delete_candidates["$version_id"]="old PR latest ($created_at) [$tags_str]"
                continue
            fi

            if has_pr_sha_tag "$tags"; then
                delete_candidates["$version_id"]="old PR SHA ($created_at) [$tags_str]"
                continue
            fi
        fi
    fi

    # Not a delete candidate, might reference other images
    # Check if this is a multi-platform manifest we should inspect
    first_tag=$(get_first_tag "$tags")
    if [[ -n "$first_tag" ]]; then
        while IFS= read -r ref_digest; do
            [[ -z "$ref_digest" ]] && continue
            # Only protect if not already marked for deletion
            if [[ -z "${delete_candidates[${digest_to_version[$ref_digest]:-}]:-}" ]]; then
                protected_digests["$ref_digest"]="referenced by $first_tag manifest"
            fi
        done <<< "$(get_referenced_digests "$REGISTRY_BASE:$first_tag")"
    fi
done

# ========== PHASE 3: FILTER DELETE CANDIDATES ==========

echo ""
echo "=== PHASE 3: FILTERING DELETE CANDIDATES ==="

# Remove from delete candidates any version whose digest is protected
final_delete_versions=()
final_delete_reasons=()

for version_id in "${!delete_candidates[@]}"; do
||||||| 1226d6d1
total_to_delete=$((${#untagged_versions[@]} + ${#old_pr_versions[@]} + ${#old_pr_sha_versions[@]}))
=======
declare -A protected_digests # sha256 -> reason

# Track versions by category
declare -A protected_versions # version_id -> reason
declare -A delete_candidates  # version_id -> reason

for version_id in "${all_version_ids[@]}"; do
    tags="${version_tags[$version_id]}"
    created_at="${version_created[$version_id]}"
    digest="${version_digest[$version_id]:-}"

    # Check for permanently protected tags
    if has_edge_tag "$tags" || has_latest_tag "$tags"; then
        protected_versions["$version_id"]="has edge/latest tag"

        if [[ -n "$digest" ]]; then
            protected_digests["$digest"]="referenced by edge/latest tagged image"
        fi

        # Fetch manifest to protect referenced platform-specific images
        first_tag=$(get_first_tag "$tags")
        if [[ -n "$first_tag" ]]; then
            echo "Inspecting manifest for protected image: $REGISTRY_BASE:$first_tag"
            while IFS= read -r ref_digest; do
                [[ -z "$ref_digest" ]] && continue
                protected_digests["$ref_digest"]="referenced by $first_tag manifest"
                echo "  Protected platform-specific digest: ${ref_digest:0:12}..."
            done <<< "$(get_referenced_digests "$REGISTRY_BASE:$first_tag")"
        fi
        continue
    fi

    # Check for attestations - handle separately later
    if has_attestation_tag "$tags"; then
        continue
    fi

    # Check for untagged versions
    if echo "$tags" | jq --exit-status '. == []' > /dev/null 2>&1; then
        delete_candidates["$version_id"]="untagged"
        continue
    fi

    # Check for old PR images
    if [[ "$CLEANUP_PR_IMAGES" == "true" ]]; then
        if is_older_than_days "$created_at" "$PR_IMAGE_AGE_DAYS"; then
            tags_str=$(echo "$tags" | jq --raw-output '.[]' | tr '\n' ',' | sed 's/,$//')

            if has_pr_latest_tag "$tags"; then
                delete_candidates["$version_id"]="old PR latest ($created_at) [$tags_str]"
                continue
            fi

            if has_pr_sha_tag "$tags"; then
                delete_candidates["$version_id"]="old PR SHA ($created_at) [$tags_str]"
                continue
            fi
        fi
    fi

    # Not a delete candidate, might reference other images
    # Check if this is a multi-platform manifest we should inspect
    first_tag=$(get_first_tag "$tags")
    if [[ -n "$first_tag" ]]; then
        while IFS= read -r ref_digest; do
            [[ -z "$ref_digest" ]] && continue
            # Only protect if not already marked for deletion
            ref_version_id="${digest_to_version[$ref_digest]:-}"
            if [[ -z "$ref_version_id" ]] || [[ -z "${delete_candidates[$ref_version_id]:-}" ]]; then
                protected_digests["$ref_digest"]="referenced by $first_tag manifest"
            fi
        done <<< "$(get_referenced_digests "$REGISTRY_BASE:$first_tag")"
    fi
done

# ========== PHASE 3: FILTER DELETE CANDIDATES ==========

echo ""
echo "=== PHASE 3: FILTERING DELETE CANDIDATES ==="

# Remove from delete candidates any version whose digest is protected
final_delete_versions=()
final_delete_reasons=()

# Sort delete candidate keys for consistent output
sorted_candidates=($(printf '%s\n' "${!delete_candidates[@]}" | sort --numeric-sort))

for version_id in "${sorted_candidates[@]}"; do
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
    digest="${version_digest[$version_id]:-}"
    reason="${delete_candidates[$version_id]}"

    if [[ -n "$digest" && -n "${protected_digests[$digest]:-}" ]]; then
        echo "Skipping $version_id ($reason): ${protected_digests[$digest]}"
        protected_versions["$version_id"]="platform-specific image: ${protected_digests[$digest]}"
        continue
    fi

    final_delete_versions+=("$version_id")
    final_delete_reasons+=("$reason")
done

# ========== PHASE 4: HANDLE ATTESTATIONS ==========

echo ""
echo "=== PHASE 4: CHECKING ATTESTATIONS ==="

orphaned_attestations=()
orphaned_attestation_reasons=()

for version_id in "${all_version_ids[@]}"; do
    tags="${version_tags[$version_id]}"

    if ! has_attestation_tag "$tags"; then
        continue
    fi

    attestation_sha=$(extract_sha256_from_attestation "$tags")
    [[ -z "$attestation_sha" ]] && continue

    # Check if the referenced image exists and is not being deleted
    ref_version_id="${digest_to_version[$attestation_sha]:-}"

    if [[ -z "$ref_version_id" ]]; then
        # Referenced image doesn't exist at all
        orphaned_attestations+=("$version_id")
        orphaned_attestation_reasons+=("attestation for non-existent sha256:${attestation_sha:0:12}...")
    else
        # Check if referenced image is being deleted
        for i in "${!final_delete_versions[@]}"; do
            if [[ "${final_delete_versions[$i]}" == "$ref_version_id" ]]; then
                orphaned_attestations+=("$version_id")
                orphaned_attestation_reasons+=("attestation for deleted sha256:${attestation_sha:0:12}...")
                break
            fi
        done
    fi
done

# ========== PHASE 5: SUMMARY AND CONFIRMATION ==========

echo ""
echo "=========================================="
echo "           CLEANUP SUMMARY"
echo "=========================================="
echo ""
echo "Protected versions:        ${#protected_versions[@]}"
echo "Versions to delete:        ${#final_delete_versions[@]}"
echo "Orphaned attestations:     ${#orphaned_attestations[@]}"
echo ""

total_to_delete=$((${#final_delete_versions[@]} + ${#orphaned_attestations[@]}))

if [[ ${#final_delete_versions[@]} -gt 0 ]]; then
    echo "--- Versions to delete ---"
    for i in "${!final_delete_versions[@]}"; do
        echo "  ${final_delete_versions[$i]}: ${final_delete_reasons[$i]}"
    done
    echo ""
fi

if [[ ${#orphaned_attestations[@]} -gt 0 ]]; then
    echo "--- Orphaned attestations to delete ---"
    for i in "${!orphaned_attestations[@]}"; do
        echo "  ${orphaned_attestations[$i]}: ${orphaned_attestation_reasons[$i]}"
    done
    echo ""
fi

if [[ $total_to_delete -eq 0 ]]; then
    echo "No versions to delete."
    exit 0
fi

if [[ "$DRY_RUN" == "true" ]]; then
    echo "[DRY RUN] Would delete $total_to_delete versions total"
fi

# ========== PHASE 6: DELETION ==========

if [[ "$DRY_RUN" == "false" && "$SKIP_CONFIRMATION" == "false" ]]; then
    echo ""
    read -p "Are you sure you want to delete $total_to_delete versions? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Deletion cancelled."
        exit 0
    fi
fi

echo ""
echo "=== STARTING DELETION ==="

deleted_count=0
failed_count=0
current=0

# Delete regular versions first
for i in "${!final_delete_versions[@]}"; do
<<<<<<< HEAD
    if delete_version "${final_delete_versions[$i]}" "${final_delete_reasons[$i]}"; then
        ((deleted_count++))
    else
        ((failed_count++))
||||||| 1226d6d1
if [[ "$skip_image_deletion" == "false" && $total_to_delete -gt 0 ]]; then
    echo ""
    echo "=== STARTING CLEANUP ==="

    # Delete untagged versions
    if [[ ${#untagged_versions[@]} -gt 0 ]]; then
        echo "Cleaning up untagged versions..."
        for version_id in "${untagged_versions[@]}"; do
            delete_version "$version_id" "untagged"

            if [[ "$DRY_RUN" == "false" ]]; then
                if [[ $? -eq 0 ]]; then
                    ((deleted_count++))
                else
                    ((failed_count++))
                fi
            fi
        done
    fi

    # Delete old PR latest versions
    if [[ ${#old_pr_versions[@]} -gt 0 ]]; then
        echo "Cleaning up old PR latest images..."
        for i in "${!old_pr_versions[@]}"; do
            delete_version "${old_pr_versions[$i]}" "${old_pr_descriptions[$i]}"

            if [[ "$DRY_RUN" == "false" ]]; then
                if [[ $? -eq 0 ]]; then
                    ((deleted_count++))
                else
                    ((failed_count++))
                fi
            fi
        done
    fi

    # Delete old PR SHA versions
    if [[ ${#old_pr_sha_versions[@]} -gt 0 ]]; then
        echo "Cleaning up old PR SHA images..."
        for i in "${!old_pr_sha_versions[@]}"; do
            delete_version "${old_pr_sha_versions[$i]}" "${old_pr_sha_descriptions[$i]}"

            if [[ "$DRY_RUN" == "false" ]]; then
                if [[ $? -eq 0 ]]; then
                    ((deleted_count++))
                else
                    ((failed_count++))
                fi
            fi
        done
    fi
fi

# ========== SECOND PASS: Find and clean up orphaned attestations ==========

echo ""
echo "=== SECOND PASS: ATTESTATION CLEANUP ==="

echo "Looking for orphaned attestations..."

# Collect all existing image SHA256s and all attestations
declare -A existing_images # sha256 -> 1
attestation_versions=()
attestation_descriptions=()
page=1

while true; do
    echo "Processing attestations page $page..."

    response=$(get_versions_page $page)

    if [[ -z "$response" ]] || echo "$response" | jq --exit-status '.message' &> /dev/null; then
        if echo "$response" | jq -e '.message' &> /dev/null; then
            echo "Error: $(echo "$response" | jq --raw-output '.message')"
            exit 1
        fi
        break
    fi

    # If no data on this page, we're done
    page_data=$(echo "$response" | jq --raw-output '.[]')
    if [[ -z "$page_data" ]]; then
        break
    fi

    # Process each version
    while IFS= read -r version; do
        if [[ -n "$version" ]]; then
            version_id=$(echo "$version" | jq --raw-output '.id')
            tags=$(echo "$version" | jq '.metadata.container.tags')
            image_name=$(echo "$version" | jq --raw-output '.name // empty')

            # If this is an attestation, store it for checking
            if has_attestation_tag "$tags"; then
                sha256=$(extract_sha256_from_attestation "$tags")
                if [[ -n "$sha256" ]]; then
                    attestation_versions+=("$version_id")
                    attestation_descriptions+=("attestation for SHA256: $sha256")
                fi
            else
                # This is a regular image, store its SHA256 if available
                if [[ -n "$image_name" ]] && [[ "$image_name" =~ ^sha256:[a-f0-9]{64}$ ]]; then
                    image_sha256="${image_name#sha256:}"
                    existing_images["$image_sha256"]=1
                fi
            fi
        fi
    done <<< "$(echo "$response" | jq -c '.[]')"

    version_count=$(echo "$response" | jq '. | length')
    if [[ $version_count -lt $PER_PAGE ]]; then
        break
    fi

    ((page++))
done

# Find orphaned attestations
orphaned_attestations=()
orphaned_descriptions=()

for i in "${!attestation_versions[@]}"; do
    version_id="${attestation_versions[$i]}"
    description="${attestation_descriptions[$i]}"

    # Extract SHA256 from description
    sha256=$(echo "$description" | sed 's/.*SHA256: //')

    # Check if corresponding image exists
    if [[ -z "${existing_images[$sha256]:-}" ]]; then
        orphaned_attestations+=("$version_id")
        orphaned_descriptions+=("orphaned $description")
=======
    ((++current))
    echo "[$current/$total_to_delete] Deleting version ${final_delete_versions[$i]}..."
    if delete_version "${final_delete_versions[$i]}" "${final_delete_reasons[$i]}"; then
        ((++deleted_count)) || true
    else
        ((++failed_count)) || true
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
    fi
done

# Delete orphaned attestations
for i in "${!orphaned_attestations[@]}"; do
<<<<<<< HEAD
    if delete_version "${orphaned_attestations[$i]}" "${orphaned_attestation_reasons[$i]}"; then
        ((deleted_count++))
    else
        ((failed_count++))
||||||| 1226d6d1
if [[ ${#orphaned_attestations[@]} -eq 0 ]]; then
    echo "No orphaned attestations found."
else
    echo "Found ${#orphaned_attestations[@]} orphaned attestations"

    # Show orphaned attestations
    echo ""
    echo "Orphaned attestations:"
    for i in "${!orphaned_attestations[@]}"; do
        echo "${orphaned_attestations[$i]}: ${orphaned_descriptions[$i]}"
    done

    if [[ "$DRY_RUN" == "true" ]]; then
        echo ""
        echo "[DRY RUN MODE] The following ${#orphaned_attestations[@]} orphaned attestations would be deleted."
=======
    ((++current))
    echo "[$current/$total_to_delete] Deleting attestation ${orphaned_attestations[$i]}..."
    if delete_version "${orphaned_attestations[$i]}" "${orphaned_attestation_reasons[$i]}"; then
        ((++deleted_count)) || true
    else
        ((++failed_count)) || true
>>>>>>> c2cfdd6672f7308710e92ff8cef08b4ba4627962
    fi
done

# ========== FINAL SUMMARY ==========

echo ""
echo "=========================================="
echo "           CLEANUP COMPLETE"
echo "=========================================="
if [[ "$DRY_RUN" == "false" ]]; then
    echo "Successfully deleted: $deleted_count"
    if [[ $failed_count -gt 0 ]]; then
        echo "Failed to delete:     $failed_count"
    fi
else
    echo "[DRY RUN] Would have deleted $total_to_delete versions"
fi
