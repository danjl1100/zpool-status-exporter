# Requirements: Handle New Pool Status (Never Scanned)

## 1. Overview

### Problem Statement
When a ZFS pool is newly created, it has never been scrubbed. In this state, `zpool status` does not include a `scan:` line. Currently, the exporter treats this as "UnknownMissing" (metric value 0), which does not distinguish between a normal new pool and a genuinely problematic missing scan state.

### Motivation
New pools are a normal, healthy state and should be reported as such in monitoring systems. The current behavior groups them with genuinely unknown/missing states, which can:
- Trigger false alerts for "missing scan data"
- Obscure the difference between "never scanned (normal)" and "scan data missing (investigate)"
- Confuse users who create new pools and immediately see concerning metric values

### High-Level Goals
1. Add a new `NeverScanned` scan status to represent pools that have never been scrubbed
2. Report this state with an appropriate Prometheus metric value (40)
3. Use the existing "100 years" scrub age convention for consistency with canceled scrubs
4. Only apply this status to healthy (ONLINE) pools to avoid masking actual issues

## 2. Functional Requirements

### FR1: New ScanStatus Variant
Add a new variant to the `ScanStatus` enum:

```rust
pub(super) enum ScanStatus {
    Unrecognized,
    ScrubRepaired,
    Resilvered,
    ScrubInProgress,
    ScrubCanceled,
    NeverScanned,  // NEW: no scan line, healthy pool
}
```

### FR2: Prometheus Metric Value
The `NeverScanned` status must be assigned metric value **40** in the `ScanStatusValue` enum:

```rust
pub(crate) enum ScanStatusValue for ScanStatus {
    #[default]
    UnknownMissing => 0,
    Unrecognized => 1,
    ScrubRepaired => 10,
    Resilvered => 15,
    ScrubInProgress => 30,
    ScrubCanceled => 35,
    NeverScanned => 40,  // NEW
}
```

**Rationale for value 40:**
- Falls in "misc" category (30-49 range) alongside ScrubInProgress and ScrubCanceled
- Represents a rare edge case / transitional state
- Higher than "normal completed" states (10, 15) but not in error range (50+)

### FR3: Detection Logic
A pool should be classified as `NeverScanned` when ALL of the following conditions are met:

1. Pool state is `ONLINE` (exactly, case-sensitive)
2. No `status:` line is present in `zpool status` output
3. No `scan:` line is present in `zpool status` output

**Implementation Details:**
- The detection occurs during parsing in `parse_zfs_metrics()`
- After parsing completes, if `scan_status` is `None` AND `state` is `Some(DeviceStatus::Online)` AND `pool_status` is `None`, set `scan_status` to `Some((ScanStatus::NeverScanned, None))`
- The timestamp component remains `None` (no timestamp for never-scanned pools)

### FR4: Scrub Age Reporting
When a pool has `NeverScanned` status:
- Report scrub age as **876,000 hours** (100 years)
- Use the existing `HUNDRED_YEARS_IN_HOURS` constant
- This matches the existing behavior for `ScrubCanceled` pools

**Rationale:**
- Consistent with canceled scrubs (also have no meaningful timestamp)
- Allows alerting on "scrub too old" to work uniformly
- 100 years is clearly a sentinel value indicating "not applicable"

### FR5: Metrics Output Format
The Prometheus metrics output for a new pool named "milton" should be:

```
# HELP zpool_scan_state Scan status: UnknownMissing = 0, Unrecognized = 1, ScrubRepaired = 10, Resilvered = 15, ScrubInProgress = 30, ScrubCanceled = 35, NeverScanned = 40
# TYPE zpool_scan_state gauge
zpool_scan_state{pool="milton"} 40

# HELP zpool_scan_age Scan age in hours
# TYPE zpool_scan_age gauge
zpool_scan_age{pool="milton"} 876000
```

The HELP text must be updated to include `NeverScanned = 40` in the value documentation.

### FR6: Preserve Existing Behavior for Non-ONLINE Pools
When a pool is NOT ONLINE (e.g., DEGRADED, FAULTED) and has no scan line:
- Continue reporting `UnknownMissing` (value 0)
- Continue reporting scrub age as 876,000 hours
- This indicates a potentially problematic state requiring investigation

## 3. Non-Functional Requirements

### NFR1: Performance
- Detection logic must not add measurable overhead to parsing
- Single-pass detection during existing parsing flow (no additional file reads)

### NFR2: Backward Compatibility
- Existing pools with scan lines must report identical metrics
- Metric names and types must not change
- Only the HELP text and value range are updated

### NFR3: Code Quality
- Must comply with project safety requirements (no unwrap, no panic, no unsafe)
- Must follow existing error handling patterns (infallible enum conversions)
- Must pass `cargo clippy` with no warnings
- Must pass `cargo fmt` formatting checks

### NFR4: Reliability
- Parsing must remain robust in the face of unexpected formats
- If detection logic is uncertain, fall back to `UnknownMissing` (safe default)

## 4. Scope

### Explicitly In Scope
✅ Adding `NeverScanned` variant to `ScanStatus` enum
✅ Assigning metric value 40 to `NeverScanned`
✅ Detection logic for new ONLINE pools without scan lines
✅ Reporting 100-year scrub age for `NeverScanned` pools
✅ Updating HELP text in metrics output
✅ Adding test case for new pool (input-10-new-pool.txt)
✅ Adding test case for degraded pool without scan (validates UnknownMissing fallback)

### Explicitly Out of Scope
❌ Changing behavior for pools with existing scan lines
❌ Adding new metrics beyond scan_state and scan_age
❌ Modifying detection logic for other scan statuses
❌ Adding "scan: none requested" as a parseable line (ZFS doesn't output this)
❌ Changing the 100-year convention (remains consistent with ScrubCanceled)

### Future Considerations
- May want to add specific error-scrub support in the future (POOL_SCAN_ERRORSCRUB)
- Could consider different scrub age values to distinguish NeverScanned from ScrubCanceled (not required now)

## 5. User Stories / Use Cases

### US1: New Pool Creation
**As a** ZFS administrator
**I want** newly created pools to report a distinct, normal scan status
**So that** I don't receive false alerts about missing scan data

**Scenario:**
1. Administrator creates a new pool: `zpool create milton mirror /dev/sda /dev/sdb`
2. The exporter parses `zpool status milton`
3. Metrics show `zpool_scan_state{pool="milton"} 40` (NeverScanned)
4. Metrics show `zpool_scan_age{pool="milton"} 876000` (100 years)
5. Monitoring system recognizes this as normal for new pools

### US2: Distinguishing New Pools from Problems
**As a** monitoring system operator
**I want** to distinguish between "never scrubbed" and "missing scan data on unhealthy pool"
**So that** I can alert only on genuinely problematic conditions

**Scenario 1 - New ONLINE pool:**
- Pool state: ONLINE
- No status/action/scan lines
- Result: `zpool_scan_state` = 40 (NeverScanned) ✓ Normal

**Scenario 2 - DEGRADED pool without scan:**
- Pool state: DEGRADED
- Has status/action lines (device failure)
- No scan line
- Result: `zpool_scan_state` = 0 (UnknownMissing) ⚠️ Investigate

### US3: Monitoring Scrub Age
**As a** monitoring system
**I want** consistent scrub age reporting for "no meaningful timestamp" cases
**So that** my alerting rules work uniformly

**Scenario:**
- Alert rule: `zpool_scan_age > 720` (>30 days)
- New pool: scrub_age = 876,000 → Alert fires ✓
- Canceled scrub: scrub_age = 876,000 → Alert fires ✓
- Both require admin attention (new pool needs first scrub, canceled needs investigation)

## 6. Acceptance Criteria

### AC1: Code Implementation
- [ ] `ScanStatus::NeverScanned` variant added to enum
- [ ] `ScanStatusValue::NeverScanned => 40` added to value enum
- [ ] Detection logic implemented in `PoolMetrics` processing
- [ ] HELP text updated to include "NeverScanned = 40"
- [ ] Code compiles without errors
- [ ] `cargo clippy` passes with no warnings
- [ ] `cargo fmt` check passes

### AC2: Test Coverage
- [ ] Test case added: `tests/input/input-10-new-pool.txt` (ONLINE, no scan)
- [ ] Expected output created: `tests/input/output-10-new-pool.txt`
- [ ] Output shows `zpool_scan_state{pool="milton"} 40`
- [ ] Output shows `zpool_scan_age{pool="milton"} 876000`
- [ ] Test case added: `tests/input/input-11-degraded-no-scan.txt` (DEGRADED, no scan)
- [ ] Expected output created: `tests/input/output-11-degraded-no-scan.txt`
- [ ] Output shows `zpool_scan_state{pool="..."}` 0` (UnknownMissing)
- [ ] All existing tests continue to pass
- [ ] `cargo test` succeeds

### AC3: Integration Testing
- [ ] `cargo run -- 127.0.0.1:8976` starts successfully
- [ ] Manual test with fake-zpool showing new pool status works correctly
- [ ] Metrics endpoint returns proper Prometheus format

### AC4: Documentation
- [ ] HELP text in metrics output documents all scan state values including NeverScanned
- [ ] Code comments explain detection logic
- [ ] This REQUIREMENTS.md serves as implementation reference

## 7. Edge Cases and Error Handling

### EC1: ONLINE Pool with Status Line but No Scan
**Input:**
- state: ONLINE
- status: "Some supported features are not enabled..."
- scan: (missing)

**Expected:** `UnknownMissing` (0)
**Rationale:** Status line indicates non-OK reason code, not a pristine new pool

### EC2: ONLINE Pool with Scan Line
**Input:**
- state: ONLINE
- scan: "scrub repaired 0B in 00:00:00..."

**Expected:** Parse scan normally (ScrubRepaired)
**Rationale:** Pool has been scanned, not a new pool

### EC3: Pool State is None (Missing)
**Input:**
- state: (missing)
- scan: (missing)

**Expected:** `UnknownMissing` (0)
**Rationale:** Incomplete data, fall back to safe default

### EC4: Pool State is Unrecognized
**Input:**
- state: UNKNOWN_STATE
- scan: (missing)

**Expected:** `UnknownMissing` (0)
**Rationale:** Unknown state is not definitively healthy, use safe default

### EC5: Non-ONLINE States without Scan
For DEGRADED, FAULTED, UNAVAIL, SUSPENDED, OFFLINE, REMOVED, SPLIT pools without scan lines:

**Expected:** `UnknownMissing` (0)
**Rationale:** These pools have issues; missing scan data compounds the problem

### EC6: Multiple Pools in Single Output
**Input:**
```
pool: pool1
state: ONLINE
(no status/scan)
...
pool: pool2
state: ONLINE
scan: scrub repaired...
```

**Expected:**
- pool1: `NeverScanned` (40)
- pool2: `ScrubRepaired` (10)

**Rationale:** Detection is per-pool independent

## 8. Dependencies and Constraints

### Dependencies
- Rust toolchain (already required)
- No new external crates required
- Existing `jiff` crate for timestamp handling (no changes needed)

### Technical Constraints
- Must maintain existing enum-based pattern matching
- Must preserve infallible conversion patterns
- Detection must occur during single parse pass
- Cannot rely on ZFS outputting "scan: none requested" (this is not emitted in practice)

### ZFS Version Compatibility
- Based on OpenZFS source code analysis (zfs/cmd/zpool/zpool_main.c)
- Behavior verified against:
  - `POOL_SCAN_NONE` internal state
  - `ZPOOL_STATUS_OK` reason code
  - Scan line printing logic (lines 8951-8956, 10120-10123)
- Should work across all OpenZFS versions (behavior is long-standing)

### Project-Specific Constraints
- No `unwrap()` calls permitted
- No `panic!()` calls permitted
- No `unsafe` code permitted
- Must maintain comprehensive error context

## 9. Open Questions

**None** - all requirements have been clarified through discussion and OpenZFS source code research.

## 10. Appendices

### Appendix A: Example Input (New Pool)

**File:** `tests/input/input-10-new-pool.txt`
```
TEST_TIMESTAMP=0
  pool: milton
 state: ONLINE
config:

	NAME                                 STATE     READ WRITE CKSUM
	milton                               ONLINE       0     0     0
	  mirror-0                           ONLINE       0     0     0
	    ata-ST8000VN004-2M2101_WSD4EYEW  ONLINE       0     0     0
	    ata-ST8000VN004-2M2101_WSD49ZDC  ONLINE       0     0     0

errors: No known data errors
```

**Key characteristics:**
- ✅ `state: ONLINE`
- ❌ No `status:` line
- ❌ No `action:` line
- ❌ No `see:` line
- ❌ No `scan:` line
- ✅ Has `config:` and device tree
- ✅ Has `errors:` line

### Appendix B: Expected Output (New Pool)

**File:** `tests/input/output-10-new-pool.txt`
```
# HELP zpool_pool_state Pool state: UnknownMissing = 0, Unrecognized = 1, Online = 10, Offline = 25, Split = 26, Degraded = 50, Faulted = 60, Suspended = 70, Removed = 80, Unavail = 100
# TYPE zpool_pool_state gauge
zpool_pool_state{pool="milton"} 10

# HELP zpool_pool_status_desc Pool status description: Normal = 0, Unrecognized = 1, FeaturesAvailable = 5, SufficientReplicasForMissing = 10, DeviceRemoved = 15, DataCorruption = 50
# TYPE zpool_pool_status_desc gauge
zpool_pool_status_desc{pool="milton"} 0

# HELP zpool_scan_state Scan status: UnknownMissing = 0, Unrecognized = 1, ScrubRepaired = 10, Resilvered = 15, ScrubInProgress = 30, ScrubCanceled = 35, NeverScanned = 40
# TYPE zpool_scan_state gauge
zpool_scan_state{pool="milton"} 40

# HELP zpool_scan_age Scan age in hours
# TYPE zpool_scan_age gauge
zpool_scan_age{pool="milton"} 876000

# HELP zpool_error_state Error status: UnknownMissing = 0, Unrecognized = 1, Ok = 10, DataErrors = 50
# TYPE zpool_error_state gauge
zpool_error_state{pool="milton"} 10

# HELP zpool_dev_state Device state: UnknownMissing = 0, Unrecognized = 1, Online = 10, Offline = 25, Split = 26, Degraded = 50, Faulted = 60, Suspended = 70, Removed = 80, Unavail = 100
# TYPE zpool_dev_state gauge
zpool_dev_state{pool="milton",dev="mirror-0"} 10
zpool_dev_state{pool="milton",dev="mirror-0/ata-ST8000VN004-2M2101_WSD4EYEW"} 10
zpool_dev_state{pool="milton",dev="mirror-0/ata-ST8000VN004-2M2101_WSD49ZDC"} 10

# HELP zpool_dev_errors_read Read error count
# TYPE zpool_dev_errors_read gauge
zpool_dev_errors_read{pool="milton",dev="mirror-0"} 0
zpool_dev_errors_read{pool="milton",dev="mirror-0/ata-ST8000VN004-2M2101_WSD4EYEW"} 0
zpool_dev_errors_read{pool="milton",dev="mirror-0/ata-ST8000VN004-2M2101_WSD49ZDC"} 0

# HELP zpool_dev_errors_write Write error count
# TYPE zpool_dev_errors_write gauge
zpool_dev_errors_write{pool="milton",dev="mirror-0"} 0
zpool_dev_errors_write{pool="milton",dev="mirror-0/ata-ST8000VN004-2M2101_WSD4EYEW"} 0
zpool_dev_errors_write{pool="milton",dev="mirror-0/ata-ST8000VN004-2M2101_WSD49ZDC"} 0

# HELP zpool_dev_errors_checksum Checksum error count
# TYPE zpool_dev_errors_checksum gauge
zpool_dev_errors_checksum{pool="milton",dev="mirror-0"} 0
zpool_dev_errors_checksum{pool="milton",dev="mirror-0/ata-ST8000VN004-2M2101_WSD4EYEW"} 0
zpool_dev_errors_checksum{pool="milton",dev="mirror-0/ata-ST8000VN004-2M2101_WSD49ZDC"} 0
```

**Key characteristics:**
- `zpool_scan_state{pool="milton"} 40` ← NeverScanned
- `zpool_scan_age{pool="milton"} 876000` ← 100 years
- HELP text includes "NeverScanned = 40"
- All other metrics normal for healthy pool

### Appendix C: Example Input (Degraded Pool, No Scan)

**File:** `tests/input/input-11-degraded-no-scan.txt`
```
TEST_TIMESTAMP=0
  pool: broken
 state: DEGRADED
status: One or more devices could not be used because the label is missing or
	invalid.  Sufficient replicas exist for the pool to continue
	functioning in a degraded state.
action: Replace the device using 'zpool replace'.
   see: https://openzfs.github.io/openzfs-docs/msg/ZFS-8000-4J
config:

	NAME        STATE     READ WRITE CKSUM
	broken      DEGRADED     0     0     0
	  mirror-0  DEGRADED     0     0     0
	    loop0   ONLINE       0     0     0
	    loop1   UNAVAIL      0     0     0  corrupted data

errors: No known data errors
```

**Key characteristics:**
- ✅ `state: DEGRADED`
- ✅ Has `status:` line (indicates problem)
- ✅ Has `action:` line
- ✅ Has `see:` line
- ❌ No `scan:` line (never been scanned)

**Expected Result:** `zpool_scan_state{pool="broken"} 0` (UnknownMissing)
**Rationale:** Pool has issues; missing scan data is concerning, not normal

### Appendix D: OpenZFS Source Code References

**Detection Logic Verification:**
- File: `zfs/cmd/zpool/zpool_main.c`
- Function: `status_callback()` (lines 10992-11145)
- Function: `print_status_reason()` (lines 10504-10866)
- Function: `print_scan_scrub_resilver_status()` (lines 8938-9079)

**Key Insights:**
1. When `reason == ZPOOL_STATUS_OK`, status/action buffers remain empty → no lines printed
2. Scan line only printed when `have_scrub && scrub_start > errorscrub_start`
3. For new pools: `ps->pss_func == POOL_SCAN_NONE` → scan function not called
4. Result: ONLINE pools with `ZPOOL_STATUS_OK` have no status/action/scan lines

**Enum Reference:**
- File: `zfs/include/sys/fs/zfs.h`
- Enum: `pool_scan_func_t` (lines 1094-1100)
- Values: `POOL_SCAN_NONE`, `POOL_SCAN_SCRUB`, `POOL_SCAN_RESILVER`, `POOL_SCAN_ERRORSCRUB`, `POOL_SCAN_FUNCS`

### Appendix E: Detection Logic Pseudo-code

```rust
// After parsing all lines for a pool:
fn finalize_pool_metrics(pool: &mut PoolMetrics) {
    // Only apply NeverScanned to ONLINE pools without status/scan
    if pool.state == Some(DeviceStatus::Online)
       && pool.pool_status.is_none()
       && pool.scan_status.is_none()
    {
        // This is a new, never-scanned pool
        pool.scan_status = Some((ScanStatus::NeverScanned, None));
    }

    // All other cases keep existing behavior:
    // - scan_status already set → use it
    // - scan_status None but has pool_status → keep as None (becomes UnknownMissing=0)
    // - scan_status None and not ONLINE → keep as None (becomes UnknownMissing=0)
}

// In metrics formatting:
fn format_scan_age(scan_status: Option<(ScanStatus, Option<Timestamp>)>, now: Timestamp) -> f64 {
    match scan_status {
        Some((_, Some(timestamp))) => (now - timestamp).hours(),
        Some((ScanStatus::ScrubCanceled, None)) => HUNDRED_YEARS_IN_HOURS,
        Some((ScanStatus::NeverScanned, None)) => HUNDRED_YEARS_IN_HOURS,  // NEW
        _ => HUNDRED_YEARS_IN_HOURS,  // None or other missing timestamp cases
    }
}
```

### Appendix F: Metric Value Ranges

```
Pool State Values:
  0   = UnknownMissing (no state line)
  1   = Unrecognized
  10  = Online          ← Healthy
  25+ = Various issues

Pool Status Description Values:
  0  = Normal (no status line)  ← Healthy
  1  = Unrecognized
  5+ = Various conditions

Scan State Values:
  0  = UnknownMissing (no scan line, not ONLINE)
  1  = Unrecognized (scan line exists but unknown format)
  10 = ScrubRepaired   ← Healthy
  15 = Resilvered      ← Healthy
  30 = ScrubInProgress ← Misc
  35 = ScrubCanceled   ← Misc
  40 = NeverScanned    ← Misc (NEW)

Error State Values:
  0  = UnknownMissing
  1  = Unrecognized
  10 = Ok              ← Healthy
  50 = DataErrors      ← Problem
```

**Design Principle:** Lower values (0-20) = normal/healthy, higher values (50+) = problems requiring attention
