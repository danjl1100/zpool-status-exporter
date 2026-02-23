#!/usr/bin/env bash
set -euo pipefail

# accept-pr.sh - Automate PR acceptance workflow
# Usage: ./accept-pr.sh [--dry-run]
#
# This script must be run from a feature branch (not main/master).
# It validates readiness, moves documentation to pr/NN-feature-name/,
# and merges the feature branch to main.

# Configuration
REPO_ROOT="$(git rev-parse --show-toplevel)"
readonly REPO_ROOT
readonly REQUIRED_DOCS=("REQUIREMENTS.md" "SPEC.md" "COMMENTS.md")
readonly PR_DIR="pr"
readonly READY_MARKER="READY TO MERGE"

# Global variables
CURRENT_BRANCH=""
DRY_RUN=false

# Error handling
die() {
    echo "ERROR: $*" >&2
    exit 1
}

# Verify we're on a feature branch (not main/master)
verify_on_feature_branch() {
    if [[ "$CURRENT_BRANCH" == "main" || "$CURRENT_BRANCH" == "master" ]]; then
        die "Cannot run from main/master branch. Switch to feature branch first."
    fi
}

# Check git working tree is clean
check_git_clean() {
    git diff-index --quiet HEAD -- || \
        die "Working tree has uncommitted changes"
}

# Verify all required documentation files exist
verify_docs_exist() {
    local missing=()
    for doc in "${REQUIRED_DOCS[@]}"; do
        [[ -f "$doc" ]] || missing+=("$doc")
    done
    [[ ${#missing[@]} -eq 0 ]] || \
        die "Missing documentation files: ${missing[*]}"
}

# Verify COMMENTS.md contains READY TO MERGE marker
verify_ready_to_merge() {
    grep -q "$READY_MARKER" COMMENTS.md || \
        die "COMMENTS.md does not contain '$READY_MARKER' status"
}

# Get main branch name (main or master)
get_main_branch() {
    if git rev-parse --verify main >/dev/null 2>&1; then
        echo "main"
    elif git rev-parse --verify master >/dev/null 2>&1; then
        echo "master"
    else
        die "Neither main nor master branch found"
    fi
}

# Determine next PR number by scanning pr/ directory
get_next_pr_number() {
    local max_num=0
    local num

    # Scan pr/ directory for NN-* pattern
    while IFS= read -r -d '' dir; do
        # Extract number from pr/NN-feature-name/
        num=$(basename "$dir" | grep -oE '^[0-9]+' || true)
        if [[ -n "$num" ]] && [[ "$num" -gt "$max_num" ]]; then
            max_num=$num
        fi
    done < <(find "$PR_DIR" -mindepth 1 -maxdepth 1 -type d -name '[0-9][0-9]-*' -print0 2>/dev/null || true)

    # Increment and format with leading zero
    printf "%02d" $((max_num + 1))
}

# Extract feature name from branch name
extract_feature_name() {
    local branch="$1"
    local name="$branch"

    # Remove common prefixes
    name="${name#feature/}"
    name="${name#tmp/}"

    # Validate non-empty
    [[ -n "$name" ]] || die "Cannot extract feature name from branch: $branch"

    echo "$name"
}

# Archive documentation to PR folder
archive_docs() {
    local pr_folder="$1"

    # Create pr folder
    mkdir -p "$pr_folder" || die "Failed to create $pr_folder"

    # Move documentation files
    git mv REQUIREMENTS.md SPEC.md COMMENTS.md "$pr_folder/" || \
        die "Failed to move documentation files"

    # Commit changes
    git commit -m "move feature docs to $pr_folder" || \
        die "Failed to commit documentation move"
}

# Merge feature branch to main
merge_to_main() {
    local feature_branch="$1"
    local pr_folder="$2"
    local main_branch="$3"

    # Switch to main
    git checkout "$main_branch" || die "Failed to checkout $main_branch branch"

    # Merge with merge commit (--no-ff)
    git merge --no-ff "$feature_branch" -m "Merge branch '$feature_branch' into $main_branch

Feature: $pr_folder
See $pr_folder/COMMENTS.md for review details." || \
        die "Failed to merge $feature_branch into $main_branch"
}

# Main execution logic
main() {
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --dry-run)
                DRY_RUN=true
                shift
                ;;
            *)
                echo "Unknown argument: $1" >&2
                echo "Usage: $0 [--dry-run]" >&2
                exit 1
                ;;
        esac
    done

    # Change to repository root
    cd "$REPO_ROOT" || die "Failed to change to repository root"

    # 1. Pre-flight checks
    echo "=== Pre-flight Checks ==="

    verify_on_feature_branch
    echo "✓ Running on feature branch: $CURRENT_BRANCH"

    check_git_clean
    echo "✓ Working tree is clean"

    verify_docs_exist
    echo "✓ All documentation files present"

    verify_ready_to_merge
    echo "✓ COMMENTS.md marked as $READY_MARKER"

    MAIN_BRANCH=$(get_main_branch)
    echo "✓ Main branch exists: $MAIN_BRANCH"

    # 2. Determine PR number and folder name
    echo ""
    echo "=== Determining PR Number ==="

    PR_NUMBER=$(get_next_pr_number)
    echo "Next PR number: $PR_NUMBER"

    FEATURE_NAME=$(extract_feature_name "$CURRENT_BRANCH")
    echo "Feature name: $FEATURE_NAME"

    PR_FOLDER="$PR_DIR/$PR_NUMBER-$FEATURE_NAME"
    echo "Target folder: $PR_FOLDER"

    # Check folder doesn't already exist
    [[ -d "$PR_FOLDER" ]] && die "PR folder already exists: $PR_FOLDER"

    # 3. Dry-run mode - stop here
    if [[ "$DRY_RUN" == "true" ]]; then
        echo ""
        echo "=== DRY RUN MODE ==="
        echo "Would perform:"
        echo "  1. Create: $PR_FOLDER/"
        echo "  2. Move: REQUIREMENTS.md SPEC.md COMMENTS.md -> $PR_FOLDER/"
        echo "  3. Commit: 'move feature docs to $PR_FOLDER'"
        echo "  4. Checkout: $MAIN_BRANCH"
        echo "  5. Merge: $CURRENT_BRANCH -> $MAIN_BRANCH (--no-ff)"
        echo ""
        echo "Dry run complete. No changes made."
        exit 0
    fi

    # 4. Archive documentation
    echo ""
    echo "=== Archiving Documentation ==="
    archive_docs "$PR_FOLDER"
    echo "✓ Documentation moved and committed"

    # 5. Merge to main
    echo ""
    echo "=== Merging to Main ==="
    merge_to_main "$CURRENT_BRANCH" "$PR_FOLDER" "$MAIN_BRANCH"
    echo "✓ Feature branch merged to $MAIN_BRANCH"

    # 6. Success summary
    echo ""
    echo "=== SUCCESS ==="
    echo "PR acceptance complete!"
    echo ""
    echo "Summary:"
    echo "  Feature branch: $CURRENT_BRANCH"
    echo "  Documentation: $PR_FOLDER/"
    echo "  Current branch: $MAIN_BRANCH"
    echo ""
    echo "Next steps:"
    echo "  - Review merge with: git log --oneline -n 10"
    echo "  - Delete feature branch with: git branch -d $CURRENT_BRANCH"
}

# Capture current branch before any operations
CURRENT_BRANCH=$(git branch --show-current)

# Execute main workflow
main "$@"

# =============================================================================
# VALIDATION PLAN
# =============================================================================
#
# Use this process to validate the script after making changes.
#
# 1. CODE QUALITY CHECKS
# -----------------------
#
#   # Verify compliance with shellcheck (must pass with no warnings)
#   $ shellcheck ./accept-pr.sh
#
#   # Check for trailing whitespace (should find none)
#   $ grep -n ' $' accept-pr.sh || echo "No trailing whitespace found"
#
#
# 2. INTEGRATION TESTING (in /tmp)
# ---------------------------------
#
#   # Create test repository
#   rm -rf /tmp/test-accept-pr
#   mkdir /tmp/test-accept-pr
#   cd /tmp/test-accept-pr
#   git init
#   git checkout -b main
#
#   # Initial commit
#   echo "Test Repository" > README.md
#   git add README.md
#   git commit -m "Initial commit"
#
#   # Create pr/ directory structure
#   mkdir pr
#   cat > pr/README.md << 'EOF'
# This folder holds information for merged pull requests.
#
# Each subfolder contains information for one pull request:
# - REQUIREMENTS.md - initial requirements from the analyst
# - SPEC.md - implementation plan from the solution architect
# - COMMENTS.md - final comments from the reviewer
#     - frontmatter contains the feature branch name
# EOF
#   git add pr/
#   git commit -m "Add pr folder"
#
#   # Create feature branch with documentation
#   git checkout -b feature/test-feature
#   cat > REQUIREMENTS.md << 'EOF'
# # Requirements
# Test requirements for validating accept-pr.sh
# EOF
#   cat > SPEC.md << 'EOF'
# # Specification
# Test specification for validating accept-pr.sh
# EOF
#   cat > COMMENTS.md << 'EOF'
# # Code Review Comments
# **Branch**: `feature/test-feature`
# **Overall Assessment**: ✅ **READY TO MERGE**
# This is a test implementation.
# EOF
#   git add REQUIREMENTS.md SPEC.md COMMENTS.md
#   git commit -m "Add documentation"
#
#   # Copy script and test
#   cp /path/to/accept-pr.sh .
#   chmod +x accept-pr.sh
#
#   # Test dry-run mode
#   ./accept-pr.sh --dry-run
#
#   # Expected dry-run output:
#   #   - Running on feature branch: feature/test-feature
#   #   - Next PR number: 01
#   #   - Feature name: test-feature
#   #   - Target folder: pr/01-test-feature
#
#   # Execute script for real
#   ./accept-pr.sh
#
#
# 3. VERIFICATION CHECKLIST
# --------------------------
#
#   # Verify documentation moved
#   ls pr/01-test-feature/
#   # Expected: COMMENTS.md  REQUIREMENTS.md  SPEC.md
#
#   # Verify root directory clean
#   ls *.md
#   # Expected: Only README.md (no REQUIREMENTS/SPEC/COMMENTS)
#
#   # Verify current branch
#   git branch --show-current
#   # Expected: main
#
#   # Verify working tree clean
#   git status
#   # Expected: Clean (except untracked accept-pr.sh)
#
#   # Verify commit history
#   git log --oneline -n 5
#   # Expected: Shows merge commit + doc move commit + original commits
#
#   # Verify merge commit message
#   git log -1 --format="%B" HEAD
#   # Expected:
#   #   Merge branch 'feature/test-feature' into main
#   #
#   #   Feature: pr/01-test-feature
#   #   See pr/01-test-feature/COMMENTS.md for review details.
#
#   # Verify commit graph structure
#   git log --oneline --graph -n 6
#   # Expected: Shows proper merge with --no-ff (two parent commits)
#
#
# 4. TEST PR NUMBER AUTO-INCREMENT
# ---------------------------------
#
#   # Create second feature branch
#   git checkout -b feature/another-feature
#   cat > REQUIREMENTS.md << 'EOF'
# # Requirements for Feature 2
# EOF
#   cat > SPEC.md << 'EOF'
# # Spec for Feature 2
# EOF
#   cat > COMMENTS.md << 'EOF'
# # Review
# ✅ **READY TO MERGE**
# EOF
#   git add .
#   git commit -m "Add second feature"
#
#   # Test that PR number increments
#   ./accept-pr.sh --dry-run
#   # Expected: Next PR number: 02, Target folder: pr/02-another-feature
#
#
# 5. ERROR CONDITION TESTS
# -------------------------
#
#   # Test: Error when run from main branch
#   git checkout main
#   ./accept-pr.sh
#   # Expected: "ERROR: Cannot run from main/master branch"
#
#   # Test: Error when READY TO MERGE missing
#   git checkout -b feature/not-ready
#   echo "# Comments" > COMMENTS.md
#   echo "# Requirements" > REQUIREMENTS.md
#   echo "# Spec" > SPEC.md
#   git add .
#   git commit -m "Not ready"
#   ./accept-pr.sh
#   # Expected: "ERROR: COMMENTS.md does not contain 'READY TO MERGE' status"
#
#   # Test: Error when documentation files missing
#   git checkout -b feature/incomplete
#   echo "test" > somefile.txt
#   git add .
#   git commit -m "Incomplete"
#   ./accept-pr.sh
#   # Expected: "ERROR: Missing documentation files: REQUIREMENTS.md SPEC.md COMMENTS.md"
#
#
# All tests should pass before considering the script validated.
