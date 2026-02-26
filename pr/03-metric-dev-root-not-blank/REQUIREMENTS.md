# Requirements: Fix Empty Device Label for Pool Root

## 1. Overview

### Problem Statement
The zpool-status-exporter currently emits device-level metrics with empty device labels (`dev=""`) for pool root devices. This occurs for all pools and affects all four device-level metrics (state, errors_read, errors_write, errors_checksum).

Empty Prometheus label values are:
- Unusual and potentially confusing in monitoring systems
- Ambiguous in their semantic meaning
- May cause issues with some Prometheus query patterns

### Motivation
- **Clarity**: Make the metric structure explicit and self-documenting
- **Best Practices**: Follow Prometheus conventions for label values
- **Usability**: Make queries and alerts easier to write and understand
- **Correctness**: Preserve critical pool-level error count data

### High-Level Goals
1. Replace empty device labels (`dev=""`) with an explicit marker (`dev="__root__"`)
2. Maintain Prometheus label consistency (all metrics have dev label)
3. Preserve all existing metric data, especially pool-level error counts
4. Update documentation to explain the semantic meaning
5. Ensure comprehensive test coverage

## 2. Functional Requirements

### FR1: Pool Root Device Label
**Requirement**: All device metrics for pool root devices (depth=0 in the device tree) MUST use the label value `dev="__root__"` instead of `dev=""`.

**Applies to**:
- `zpool_dev_state{pool="...",dev="__root__"}`
- `zpool_dev_errors_read{pool="...",dev="__root__"}`
- `zpool_dev_errors_write{pool="...",dev="__root__"}`
- `zpool_dev_errors_checksum{pool="...",dev="__root__"}`

**Rationale**: The value `__root__` follows Prometheus naming conventions (double underscore prefix for special/internal labels) and clearly indicates this represents the pool root, not a child device.

### FR2: Label Consistency
**Requirement**: The `dev` label MUST be present on ALL device metrics, including pool root devices. No metrics should omit the `dev` label.

**Rationale**: Prometheus requires consistent label names across all instances of a metric. Having some instances with `dev` and others without violates the data model.

### FR3: Multi-Pool Support
**Requirement**: When multiple pools are present, each pool MUST have its own independent `dev="__root__"` entry:

```
zpool_dev_state{pool="pool1",dev="__root__"} 10
zpool_dev_state{pool="pool1",dev="mirror-0"} 10
zpool_dev_state{pool="pool2",dev="__root__"} 50
zpool_dev_state{pool="pool2",dev="raidz1-0"} 50
```

**Rationale**: Pool root metrics are per-pool and independent.

### FR4: Value Consistency with Pool State
**Requirement**: The value of `zpool_dev_state{pool="X",dev="__root__"}` MUST match the value of `zpool_pool_state{pool="X"}` since both represent the overall pool status.

**Rationale**: These metrics represent the same underlying state and must remain synchronized.

### FR5: Preserve Error Count Data
**Requirement**: Pool-level error counts (currently at `dev=""`) MUST be preserved at `dev="__root__"`. These represent aggregate error counts for the pool and are critical for monitoring.

**Rationale**: User has identified these aggregate error counts as critical for their monitoring strategy.

### FR6: Child Device Labels Unchanged
**Requirement**: Device labels for non-root devices (depth >= 1) MUST NOT change. They should continue using the current slash-separated hierarchy format:
- `dev="mirror-0"`
- `dev="mirror-0/loop0"`
- `dev="raidz1-0/sda1"`

**Rationale**: Only the pool root (depth=0) behavior is changing. Child device labels work correctly.

## 3. Non-Functional Requirements

### NFR1: Performance
**Requirement**: The change MUST NOT introduce measurable performance degradation in metrics generation or parsing.

**Rationale**: This is a string substitution change that should have negligible performance impact.

### NFR2: Code Quality
**Requirement**: Implementation MUST adhere to all project code quality standards:
- `cargo clippy` passes with no warnings
- `cargo fmt` passes
- No use of `unwrap()`, `panic!()`, or `unsafe` code
- All public items documented
- Proper error handling where applicable

**Rationale**: Project CLAUDE.md specifies strict safety requirements.

### NFR3: Backward Compatibility
**Requirement**: This is explicitly a **BREAKING CHANGE**. No backward compatibility is required.

**Impact**: Existing Prometheus queries, dashboards, and alerts that filter on `dev=""` will need to be updated to use `dev="__root__"`.

**Rationale**: User has accepted this as a breaking change given the project's early stage and the importance of correctness.

## 4. Scope

### In Scope
- Changing `dev=""` to `dev="__root__"` for pool root devices
- Updating all test fixtures to reflect the new behavior
- Adding explicit test case for `__root__` validation
- Updating Prometheus HELP text for affected metrics
- Updating README documentation
- Bumping crate version in Cargo.toml

### Explicitly Out of Scope
- Other Prometheus compliance improvements (tracked separately for future PR)
- Backward compatibility support (transition period, config flags, dual output)
- CHANGELOG or formal release notes (may be handled separately)
- Changes to child device label format
- Changes to pool-level metrics (non-device metrics)

### Future Considerations
- Full Prometheus compliance audit (separate PR)
- Validation of other metric label formats
- Runtime validation to prevent empty labels

## 5. User Stories / Use Cases

### US1: Monitoring Pool Root Status
**As a** systems administrator
**I want** to query the status of pool root devices explicitly
**So that** I can monitor overall pool health without ambiguous empty labels

**Acceptance**: Query `zpool_dev_state{dev="__root__"}` returns all pool root status values with clear, non-empty labels.

### US2: Monitoring Pool-Level Errors
**As a** systems administrator
**I want** to track aggregate error counts at the pool level
**So that** I can alert on pool-level error thresholds

**Acceptance**: Queries like `zpool_dev_errors_read{dev="__root__"}` return pool-level aggregate error counts with explicit labels.

### US3: Comparing Pool State Metrics
**As a** monitoring system developer
**I want** to verify consistency between pool state and device state metrics
**So that** I can ensure data integrity

**Acceptance**: For any pool X, `zpool_pool_state{pool="X"}` equals `zpool_dev_state{pool="X",dev="__root__"}`.

### US4: Updating Existing Queries
**As a** existing user of the exporter
**I want** clear documentation on what changed
**So that** I can update my dashboards and alerts

**Acceptance**: README documents that `dev=""` has changed to `dev="__root__"` and explains how to update queries.

### US5: Understanding Metric Structure
**As a** new user of the exporter
**I want** to understand what `dev="__root__"` means
**So that** I can write correct queries and alerts

**Acceptance**: Prometheus HELP text and README both explain that `dev="__root__"` represents the pool root device with aggregate error counts.

## 6. Acceptance Criteria

### AC1: Code Implementation
- [ ] Empty device labels changed to `dev="__root__"` for pool root devices (depth=0)
- [ ] Change affects all four device metrics: state, errors_read, errors_write, errors_checksum
- [ ] Child device labels (depth >= 1) remain unchanged
- [ ] Multi-pool scenarios work correctly (each pool has its own `__root__` entry)
- [ ] Code compiles without errors
- [ ] `cargo clippy` passes with no warnings
- [ ] `cargo fmt` passes
- [ ] No new `unwrap`, `panic`, or `unsafe` code introduced

### AC2: Test Coverage - Existing Tests Updated
- [ ] All test output fixtures updated to use `dev="__root__"` instead of `dev=""`
- [ ] Fixtures updated using `cargo insta` snapshot testing tool
- [ ] All existing tests pass with updated fixtures
- [ ] `cargo test` succeeds

### AC3: Test Coverage - New Test Case
- [ ] New test case added explicitly for `__root__` validation
- [ ] Test validates label format: exactly `dev="__root__"` (not empty, not pool name)
- [ ] Test validates all four metric types include `dev="__root__"` entry
- [ ] Test validates `zpool_dev_state{dev="__root__"}` matches corresponding `zpool_pool_state` value
- [ ] Test is documented and explains what it validates

### AC4: Documentation - HELP Text
- [ ] `zpool_dev_state` HELP text mentions `dev="__root__"` represents pool root
- [ ] HELP text explains that pool root metrics include aggregate error counts
- [ ] HELP text updates are automatically reflected in Prometheus output

### AC5: Documentation - README
- [ ] README includes section on metric structure
- [ ] README explains that `dev="__root__"` represents the pool root device
- [ ] README explains the semantic meaning of pool root metrics vs child device metrics
- [ ] README mentions this is a breaking change from previous behavior (if relevant)

### AC6: Version Bump
- [ ] Crate version incremented in Cargo.toml
- [ ] Version bump reflects breaking change semantics:
  - If version >= 1.0.0: Major version bump (e.g., 1.2.3 → 2.0.0)
  - If version < 1.0.0: Minor version bump (e.g., 0.2.3 → 0.3.0)

### Final Status
When all above criteria are met, the feature is complete and ready to merge.

## 7. Edge Cases and Error Handling

### EC1: Empty Device Tree
**Case**: What if parsing produces a device with no name at all (not just depth=0)?
**Expected**: This should not occur based on current parsing logic. If it does, it indicates a parsing bug that should be investigated separately.
**Handling**: Current implementation - if depth is 0, substitute `__root__`. This requirement does not change that behavior.

### EC2: Multiple Pools
**Case**: Two or more pools in single `zpool status` output
**Expected**: Each pool gets its own independent `dev="__root__"` entry
**Handling**: Works automatically since device tree processing is per-pool
**Test Coverage**: Existing test fixtures include multi-pool scenarios

### EC3: Pool Name Equals "__root__"
**Case**: What if a user names their pool "__root__"?
**Expected**: Unlikely but possible. Result would be:
```
zpool_dev_state{pool="__root__",dev="__root__"} 10
```
This is technically correct - the pool label shows the actual pool name, the dev label shows it's the pool root. Could be confusing but not incorrect.
**Handling**: No special handling required. If this becomes an issue, it can be addressed in future work.
**Impact**: Very low probability edge case.

### EC4: Depth=0 But Not Pool Root
**Case**: Could there be depth=0 devices that aren't the pool root?
**Expected**: No. Based on ZFS `zpool status` format, depth=0 is always and only the pool name itself.
**Handling**: No special handling needed. Implementation correctly treats depth=0 as pool root.

### EC5: Child Device Named "__root__"
**Case**: What if a child device (vdev) is literally named "__root__"?
**Expected**: Extremely unlikely (ZFS doesn't allow arbitrary naming of vdevs). If it occurred:
```
zpool_dev_state{pool="mypool",dev="__root__"} 10      # pool root
zpool_dev_state{pool="mypool",dev="vdev0/__root__"} 10  # child device (if possible)
```
The slash separator would distinguish them.
**Handling**: No special handling required.
**Impact**: Theoretical edge case with near-zero probability.

## 8. Dependencies and Constraints

### Technical Dependencies
- **Rust toolchain**: Must maintain compatibility with project's minimum supported Rust version
- **Prometheus format**: Output must remain valid Prometheus exposition format
- **ZFS parsing**: Relies on correct depth calculation in `zfs.rs` parsing logic

### Technical Constraints
- **Label consistency**: Prometheus requires all instances of a metric to have same labels
- **String formatting**: Implementation must produce valid quoted Prometheus label values
- **Performance**: String substitution should not measurably impact performance
- **Memory**: No significant memory overhead from storing `__root__` vs empty string

### Project Constraints
- **Code quality**: Must pass all clippy/fmt checks per project standards
- **Safety**: No unwrap/panic/unsafe per project CLAUDE.md
- **Testing**: Must use snapshot testing (insta crate) for output validation
- **Documentation**: All code must be documented per project standards

### Timeline and Priority
- **Priority**: Medium (quality improvement, not blocking production)
- **Urgency**: Non-critical, can be scheduled in normal development flow
- **Scope**: Focused change, should be relatively quick to implement

## 9. Implementation Guidance

### Architect's Discretion
The **location** of the `__root__` substitution logic is left to the architect/implementer's discretion. Possible approaches:

1. **DeviceTreeName::fmt (Debug impl)**: Output `"__root__"` when vector is empty
2. **DeviceTreeName::update**: Push `"__root__"` into vector when depth==0 instead of clearing
3. **Call site**: Check for empty and substitute at point of metric writing

Choose based on:
- Code clarity and maintainability
- Minimizing changes to existing logic
- Ease of testing and validation

### Current Code Location
The relevant code is in `src/fmt.rs`:
- `DeviceTreeName` struct (line 324)
- `DeviceTreeName::update` method (line 326) - currently clears vector when depth=0
- `DeviceTreeName::fmt` Debug impl (line 336) - formats as quoted slash-separated string

The device metric writing loop is at line 288-313, which iterates through all devices including depth=0.

### Testing Approach
1. **Update fixtures first**: Run tests, let them fail, update expected outputs
2. **Use cargo insta**: `cargo insta test` to run tests, `cargo insta review` to review changes
3. **Add explicit test**: Create new test case after fixtures are updated
4. **Validate manually**: Run `cargo run` and check actual output format

## 10. Open Questions

### Resolved Questions
- ✅ What value should replace `dev=""`? → `__root__`
- ✅ Should we maintain backward compatibility? → No, breaking change accepted
- ✅ Where should version bump happen? → In this PR
- ✅ What test coverage is needed? → Update all fixtures + add explicit test
- ✅ What documentation is needed? → HELP text + README
- ✅ Should code comments be added? → Not required (documentation focus is external)

### No Open Questions
All requirements have been clarified and confirmed with the user.

## 11. Appendices

### A. Example Input (from test fixtures)
```
config:

	NAME        STATE     READ WRITE CKSUM
	dummy       DEGRADED     0     0     0
	  mirror-0  ONLINE       0     0     0
	    loop0   ONLINE       0     0     0
```

### B. Current Output (Before Fix)
```
zpool_dev_state{pool="dummy",dev=""} 50
zpool_dev_state{pool="dummy",dev="mirror-0"} 10
zpool_dev_state{pool="dummy",dev="mirror-0/loop0"} 10
zpool_dev_errors_read{pool="dummy",dev=""} 0
zpool_dev_errors_read{pool="dummy",dev="mirror-0"} 0
zpool_dev_errors_read{pool="dummy",dev="mirror-0/loop0"} 0
```

### C. Expected Output (After Fix)
```
zpool_dev_state{pool="dummy",dev="__root__"} 50
zpool_dev_state{pool="dummy",dev="mirror-0"} 10
zpool_dev_state{pool="dummy",dev="mirror-0/loop0"} 10
zpool_dev_errors_read{pool="dummy",dev="__root__"} 0
zpool_dev_errors_read{pool="dummy",dev="mirror-0"} 0
zpool_dev_errors_read{pool="dummy",dev="mirror-0/loop0"} 0
```

### D. Example Prometheus Queries

**Before Fix:**
```promql
# Query pool root status (confusing with empty label)
zpool_dev_state{dev=""}

# Query specific pool root
zpool_dev_state{pool="mypool",dev=""}

# Query all non-root devices (awkward negation)
zpool_dev_state{dev!=""}
```

**After Fix:**
```promql
# Query pool root status (explicit)
zpool_dev_state{dev="__root__"}

# Query specific pool root
zpool_dev_state{pool="mypool",dev="__root__"}

# Query all non-root devices (explicit exclusion)
zpool_dev_state{dev!="__root__"}

# Query all devices including root
zpool_dev_state{}
```

### E. Breaking Change Migration Guide

For existing users who need to update their queries:

| Before (dev="") | After (dev="__root__") |
|-----------------|------------------------|
| `zpool_dev_state{dev=""}` | `zpool_dev_state{dev="__root__"}` |
| `zpool_dev_errors_read{dev=""}` | `zpool_dev_errors_read{dev="__root__"}` |
| `zpool_dev_errors_write{dev=""}` | `zpool_dev_errors_write{dev="__root__"}` |
| `zpool_dev_errors_checksum{dev=""}` | `zpool_dev_errors_checksum{dev="__root__"}` |

**Alert Rules**: Update any alert rules that filter on `dev=""` to use `dev="__root__"` instead.

**Dashboards**: Update Grafana or other dashboard queries to use the new label value.

---

## Document Metadata

**Date Created**: 2026-02-25
**Requirements Analyst**: Claude (AI Requirements Analyst)
**Status**: Complete - Ready for Architecture & Implementation
**Related Files**:
- `pr/01-pool-never-scanned/COMMENTS.md` (original issue identification)
- `src/fmt.rs` (implementation location)
- `src/zfs.rs` (parsing logic)
- `tests/input/*.txt` (test fixtures)

**Review Status**: Requirements reviewed and confirmed with user through structured interview.
