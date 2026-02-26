# Specification: NixOS VM Test for NeverScanned Pool Status

## Summary

Create a NixOS VM test that validates the "NeverScanned" feature works end-to-end in a real ZFS environment. The test will create a new mirror pool using file-backed disk images, verify the pool is in a healthy ONLINE state with no scan history, and validate that the zpool-status-exporter correctly reports the pool with `scan_state=40` (NeverScanned) and `scan_age=876000` (100 years) via the Prometheus metrics endpoint.

## Requirements Reference

This specification implements all requirements from `REQUIREMENTS.md` for the NixOS VM test (Phase 6 of the pool-never-scanned feature).

**Key requirements:**
- **FR1-FR2**: Create test file with proper NixOS configuration
- **FR3**: Service startup validation
- **FR4-FR5**: Pool creation and precondition validation
- **FR6**: Metrics validation with retry logic
- **FR7**: No cleanup needed (ephemeral VM)
- **FR8**: Integration with test suite

## Goals

- ✅ Validate NeverScanned feature in production-like environment
- ✅ Test full stack integration (NixOS module, systemd service, ZFS kernel module, HTTP endpoint)
- ✅ Provide regression protection for future changes
- ✅ Enable interactive debugging via driverInteractive mode

## Non-Goals

- ❌ Testing other scan states (covered by Rust tests)
- ❌ Testing pool state transitions
- ❌ Testing authentication (separate test exists)
- ❌ Testing multiple pools in one test
- ❌ Modifying exporter code

---

## Design Decisions

### Approach

Implement a minimal VM test following the established patterns in `empty-zfs.nix` and `empty-zfs-auth.nix`. The test will:

1. Boot a NixOS VM with ZFS support and zpool-status-exporter service
2. Create a file-backed mirror pool to simulate a newly created pool
3. Verify preconditions (ONLINE state, no scan line)
4. Use `wait_until_succeeds` to robustly handle timing between pool creation and detection
5. Validate three metrics: scan_state, scan_age, and HELP text

**Key design decisions:**
- **Pool name**: `newpool` - simple, clear, matches test file naming
- **Retry mechanism**: `wait_until_succeeds()` - NixOS built-in, handles timing issues robustly
- **Host ID**: `abcd1234` - unique, easy to remember, different pattern from existing tests
- **Disk size**: 64MB - minimal, fast, can increase if ZFS rejects it

### Alternatives Considered

#### Alternative 1: Simple sleep instead of wait_until_succeeds
**Pros:** Simpler code, fewer lines
**Cons:** Fixed delay may be too short on slow systems or wasteful on fast systems
**Why rejected:** `wait_until_succeeds` is more robust and is standard practice in NixOS tests

#### Alternative 2: No retry logic
**Pros:** Fastest execution, simplest code
**Cons:** Race condition risk - ZFS may need moment to update state after pool creation
**Why rejected:** Requirements explicitly call for retry/wait logic (FR6)

#### Alternative 3: Larger disk images (128MB or 256MB)
**Pros:** Less likely to hit ZFS minimum size limits
**Cons:** Slower test execution, more disk I/O
**Why rejected:** Requirements suggest starting with 64MB and increasing if needed (FR4). We optimize for speed first.

#### Alternative 4: Test multiple pools
**Pros:** More comprehensive testing
**Cons:** Complexity, longer execution time, scope creep
**Why rejected:** Explicitly out of scope per requirements. Keep test focused on single happy path.

### Justification

The chosen approach balances simplicity, robustness, and adherence to existing patterns. Using `wait_until_succeeds` provides reliability without complexity, and 64MB disks optimize for fast test execution while allowing for adjustment if needed. The test structure exactly mirrors existing VM tests, ensuring consistency and maintainability.

---

## Architecture

### Component Overview

```
┌─────────────────────────────────────────┐
│ NixOS VM Test (new-pool-never-scanned)  │
└─────────────────────────────────────────┘
                    │
        ┌───────────┴────────────┐
        ▼                        ▼
┌──────────────┐      ┌─────────────────────┐
│  Test File   │      │   Test Integration  │
│ (new test)   │      │   (modify existing) │
└──────────────┘      └─────────────────────┘
        │                        │
        ▼                        ▼
┌────────────────────┐  ┌──────────────────┐
│ nix/vm-tests/      │  │ nix/vm-tests/    │
│ new-pool-never-    │  │ default.nix      │
│ scanned.nix        │  │                  │
└────────────────────┘  └──────────────────┘
```

### Module Organization

**New files:**
- `nix/vm-tests/new-pool-never-scanned.nix`: Complete VM test implementation

**Modified files:**
- `nix/vm-tests/default.nix`: Add test to `test_sources` attribute set

**No changes to:**
- Exporter source code (feature already implemented)
- NixOS module (already supports required configuration)
- Other test files

### Data Flow

1. **Test initialization**:
   - NixOS VM boots with ZFS support enabled
   - zpool-status-exporter.service starts automatically
   - Test waits for systemd units to reach active state

2. **Pool creation**:
   - Test creates two 64MB file-backed disk images in `/tmp/`
   - Test creates mirror pool named "newpool" using `zpool create`
   - ZFS kernel module creates pool, marks it as ONLINE
   - No scan line exists (pool never scrubbed)

3. **Precondition validation**:
   - Test queries `zpool status` to verify ONLINE state
   - Test verifies no "scan:" line exists in output

4. **Metrics validation**:
   - Test uses `wait_until_succeeds` to query `/metrics` endpoint
   - Exporter executes `zpool status`, parses output
   - Exporter detects ONLINE pool with no scan line → NeverScanned (value 40)
   - Exporter formats Prometheus metrics with scan_state=40, scan_age=876000
   - Test validates all three requirements (scan_state, scan_age, HELP text)

5. **Test completion**:
   - Test succeeds if all validations pass
   - VM is destroyed (pool and disks are ephemeral)

### Integration Points

**With existing test infrastructure:**
- `nix/vm-tests/default.nix`: Uses same `test_sources` pattern
- NixOS test framework: Uses standard `pkgs.nixosTest` structure
- Build system: Integrated via `nix build .#vm-tests`

**With exporter:**
- HTTP endpoint: Standard GET request to `/metrics`
- No special configuration needed
- Uses same listen_address pattern as other tests (127.0.0.1:1234)

---

## Data Structures

### Test Parameters (Nix)

```nix
{
  pkgs,        # NixOS package set (provided by framework)
  nixosModule, # zpool-status-exporter NixOS module (provided by test suite)
}
```

### Test Configuration (Nix)

```nix
let
  listen_address = "127.0.0.1:1234";  # Standard test bind address
  poolname = "newpool";                # Test pool name
in
  pkgs.nixosTest {
    name = "new-pool-never-scanned";   # Test identifier
    nodes.machine = { ... };           # VM configuration
    testScript = '' ... '';            # Test logic (Python)
  }
```

### VM Configuration

```nix
nodes.machine = {pkgs, ...}: {
  imports = [nixosModule];                      # Import exporter module
  boot.supportedFilesystems = ["zfs"];          # Enable ZFS kernel module
  networking.hostId = "abcd1234";               # Unique 8-hex-char ID (required by ZFS)
  services.zpool-status-exporter = {
    enable = true;                              # Start exporter service
    inherit listen_address;                     # Bind to 127.0.0.1:1234
  };
};
```

---

## Interface Specifications

### Test Script (Python)

The test script runs in the NixOS test framework's Python environment. The `machine` object provides methods for controlling the VM and running commands.

**Key methods used:**

```python
# Wait for systemd unit to reach active state
machine.wait_for_unit("unit-name")

# Run command, expect success (exit code 0)
machine.succeed("command")

# Run command, expect failure (exit code != 0)
machine.fail("command")

# Retry command with 1-second intervals until success
machine.wait_until_succeeds("command")
```

### Test Script Structure

```python
# Phase 1: Wait for system to be ready
machine.wait_for_unit("default.target")
machine.wait_for_unit("zpool-status-exporter.service")

# Phase 2: Create pool
machine.succeed("dd if=/dev/zero of=/tmp/disk1.img bs=1M count=64")
machine.succeed("dd if=/dev/zero of=/tmp/disk2.img bs=1M count=64")
machine.succeed("zpool create newpool mirror /tmp/disk1.img /tmp/disk2.img")

# Phase 3: Verify preconditions
machine.succeed("zpool status newpool | grep 'state: ONLINE'")
machine.fail("zpool status newpool | grep 'scan:'")

# Phase 4: Validate metrics (with retry)
machine.wait_until_succeeds("curl http://127.0.0.1:1234/metrics | grep 'zpool_scan_state{pool=\"newpool\"} 40'")
machine.wait_until_succeeds("curl http://127.0.0.1:1234/metrics | grep 'zpool_scan_age{pool=\"newpool\"} 876000'")
machine.wait_until_succeeds("curl http://127.0.0.1:1234/metrics | grep 'NeverScanned = 40'")
```

---

## Error Handling

### Test Failure Modes

All test failures will be reported by the NixOS test framework with clear error messages indicating which step failed.

**Failure scenarios:**

1. **Service fails to start**
   - `wait_for_unit` times out
   - Error: "machine: unit zpool-status-exporter.service failed to reach state active"
   - Likely cause: Exporter configuration error, port binding issue

2. **Pool creation fails**
   - `machine.succeed("zpool create ...")` fails
   - Error: ZFS error message from zpool command
   - Likely causes: Disk images too small, ZFS module not loaded
   - Resolution: Increase disk size from 64MB to 128MB or 256MB

3. **Precondition validation fails**
   - Pool not ONLINE: `grep 'state: ONLINE'` fails
   - Error: grep returns no match
   - Likely cause: Pool created in DEGRADED or other state

   - Scan line exists: `machine.fail("grep 'scan:'")` succeeds (should fail)
   - Error: grep found a match when it shouldn't
   - Likely cause: ZFS automatically ran a scan (unusual for new pool)

4. **Metrics validation fails**
   - `wait_until_succeeds` times out (default: ~infinite retries with 1s intervals)
   - Error: Command never succeeded after many retries
   - Likely causes:
     - Exporter not running (but would have been caught by wait_for_unit)
     - Exporter encountering parse error
     - Incorrect metric value (feature regression)
   - Debug: Use `.driverInteractive` to manually inspect metrics output

### Error Recovery

**For disk size issues:**
```nix
# If 64MB fails, modify:
machine.succeed("dd if=/dev/zero of=/tmp/disk1.img bs=1M count=128")  # Was: count=64
machine.succeed("dd if=/dev/zero of=/tmp/disk2.img bs=1M count=128")  # Was: count=64
```

**For debugging failures:**
```bash
# Build interactive driver
nix build .#vm-tests.tests.new-pool-never-scanned.driverInteractive

# Run interactively
./result/bin/nixos-test-driver

# In Python REPL:
>>> start_all()
>>> machine.succeed("zpool list")
>>> machine.succeed("zpool status")
>>> machine.succeed("curl http://127.0.0.1:1234/metrics")
>>> machine.succeed("journalctl -u zpool-status-exporter")
```

### No Cleanup Needed

The test does **not** need error handling for cleanup because:
- VM is ephemeral and destroyed after test completes
- File-backed disk images are in `/tmp/` (VM-local, destroyed with VM)
- No persistent state or external resources

---

## Implementation Plan

### Phase 1: Create Test File

**File to create:** `nix/vm-tests/new-pool-never-scanned.nix`

**Actions:**
1. Create new file with standard Nix function signature
2. Define local bindings for `listen_address` and `poolname`
3. Implement `pkgs.nixosTest` structure with name
4. Configure `nodes.machine` with ZFS support and exporter service
5. Write test script with all phases (service startup, pool creation, validation)

**Testing:**
- Syntax check: `alejandra --check nix/vm-tests/new-pool-never-scanned.nix`
- Full test: `nix build .#vm-tests.tests.new-pool-never-scanned` (executes the VM test)

### Phase 2: Integrate with Test Suite

**File to modify:** `nix/vm-tests/default.nix`

**Actions:**
1. Add entry to `test_sources` attribute set:
   ```nix
   new-pool-never-scanned = ./new-pool-never-scanned.nix;
   ```
2. Maintain alphabetical ordering (after max-bind-retries)
3. No other changes needed (framework automatically includes it)

**Testing:** Run `nix build .#vm-tests` (executes all VM tests including the new one)

### Phase 3: Validation

**Actions:**
1. Run individual test: `nix build .#vm-tests.tests.new-pool-never-scanned`
2. Verify test passes (exit code 0)
3. Run all VM tests: `nix build .#vm-tests`
4. Verify no regressions in other tests

**Testing:** Check test output includes all expected validation steps

### Implementation Order

1. **First**: Create `new-pool-never-scanned.nix` with complete implementation
   - Rationale: Need working test file before integration

2. **Second**: Modify `default.nix` to add test to suite
   - Rationale: Integration depends on test file existing

3. **Third**: Run format check and validation
   - Rationale: Ensure code quality before considering complete

**Sequential dependencies:**
- Step 2 requires Step 1 (can't reference non-existent file)
- Step 3 requires Steps 1-2 (validate complete implementation)

**No parallel work:** All steps are sequential for this small change

---

## Testing Strategy

### Integration Tests

**Test file:** `nix/vm-tests/new-pool-never-scanned.nix`

This IS the integration test. The VM test itself provides end-to-end validation.

**Test scope:**
- ✅ NixOS module configuration
- ✅ Systemd service startup
- ✅ ZFS kernel module functionality
- ✅ File-backed pool creation
- ✅ Exporter HTTP endpoint
- ✅ Metrics parsing and formatting
- ✅ NeverScanned feature detection

**Test scenarios:**

1. **Service Startup**
   - Verify systemd brings service to active state
   - Verify service binds to configured address

2. **Pool Creation**
   - Verify `dd` creates disk images successfully
   - Verify `zpool create` succeeds with mirror configuration
   - Verify pool is accessible via `zpool status`

3. **Precondition Validation**
   - Verify pool state is ONLINE
   - Verify no scan line exists in zpool status output

4. **Metrics Validation**
   - Verify scan_state metric reports value 40 (NeverScanned)
   - Verify scan_age metric reports value 876000 (100 years)
   - Verify HELP text includes "NeverScanned = 40"

### Unit Tests

**Not applicable** - this specification covers only the VM test. The exporter code already has comprehensive unit tests for the NeverScanned feature (completed in PR #01).

### Edge Cases

The VM test focuses on the happy path only. Edge cases are covered by existing Rust tests:

**Covered by Rust tests (not in VM test):**
- Degraded pool without scan line → UnknownMissing (value 0)
- Pool with status line → not NeverScanned
- Multiple pools with mixed states

**VM test edge cases:**

1. **Pool creation failure due to disk size**
   - Handled by: Implementer can increase disk size per FR4
   - Not tested: We don't test the failure case, only success

2. **Metrics not immediately available**
   - Handled by: `wait_until_succeeds` retries until success
   - Robust against timing variations

3. **Service fails to start**
   - Handled by: `wait_for_unit` will fail test clearly
   - Indicates configuration or build issue

### VM Tests (This Specification)

**Test execution:**

```bash
# Run only new test
nix build .#vm-tests.tests.new-pool-never-scanned

# Run all VM tests (includes new test)
nix build .#vm-tests

# Run interactively for debugging
nix build .#vm-tests.tests.new-pool-never-scanned.driverInteractive
./result/bin/nixos-test-driver
```

**Expected output when test passes:**

```
machine: waiting for unit default.target
machine: unit default.target has reached state active
machine: waiting for unit zpool-status-exporter.service
machine: unit zpool-status-exporter.service has reached state active
machine: must succeed: dd if=/dev/zero of=/tmp/disk1.img bs=1M count=64
64+0 records in
64+0 records out
machine: must succeed: dd if=/dev/zero of=/tmp/disk2.img bs=1M count=64
64+0 records in
64+0 records out
machine: must succeed: zpool create newpool mirror /tmp/disk1.img /tmp/disk2.img
machine: must succeed: zpool status newpool | grep 'state: ONLINE'
  state: ONLINE
machine: must fail: zpool status newpool | grep 'scan:'
machine: waiting for success: curl http://127.0.0.1:1234/metrics | grep 'zpool_scan_state{pool="newpool"} 40'
zpool_scan_state{pool="newpool"} 40
machine: waiting for success: curl http://127.0.0.1:1234/metrics | grep 'zpool_scan_age{pool="newpool"} 876000'
zpool_scan_age{pool="newpool"} 876000
machine: waiting for success: curl http://127.0.0.1:1234/metrics | grep 'NeverScanned = 40'
# HELP zpool_scan_state Scan status: UnknownMissing = 0, Unrecognized = 1, ScrubRepaired = 10, Resilvered = 15, ScrubInProgress = 30, ScrubCanceled = 35, NeverScanned = 40
test script finished in X.XXs
```

---

## Configuration & CLI Changes

**No CLI or configuration changes** - this specification only adds a test file. The exporter and NixOS module already support all required functionality.

---

## Documentation Updates

### Code Documentation

**Comments in test file:**
- Document the purpose of each test phase
- Explain the wait_until_succeeds usage (handles timing)
- Note that VM and pool are ephemeral (no cleanup needed)

Example:
```nix
testScript = ''
  # Wait for system and service to be ready
  machine.wait_for_unit("default.target")
  machine.wait_for_unit("zpool-status-exporter.service")

  # Create file-backed disk images (64MB each)
  machine.succeed("dd if=/dev/zero of=/tmp/disk1.img bs=1M count=64")
  machine.succeed("dd if=/dev/zero of=/tmp/disk2.img bs=1M count=64")

  # Create mirror pool (new pool will have no scan line)
  machine.succeed("zpool create ${poolname} mirror /tmp/disk1.img /tmp/disk2.img")

  # Verify preconditions: pool is ONLINE and has never been scanned
  machine.succeed("zpool status ${poolname} | grep 'state: ONLINE'")
  machine.fail("zpool status ${poolname} | grep 'scan:'")

  # Validate metrics (wait_until_succeeds handles timing of pool detection)
  machine.wait_until_succeeds("curl http://${listen_address}/metrics | grep 'zpool_scan_state{pool=\"${poolname}\"} 40'")
  machine.wait_until_succeeds("curl http://${listen_address}/metrics | grep 'zpool_scan_age{pool=\"${poolname}\"} 876000'")
  machine.wait_until_succeeds("curl http://${listen_address}/metrics | grep 'NeverScanned = 40'")
'';
```

### README.md Updates

**No updates needed** - VM tests are already documented. This test follows existing patterns.

### User-Facing Documentation

**No updates needed** - this is an internal test, not user-facing functionality.

---

## Edge Cases & Error Scenarios

### Edge Case 1: Disk Images Too Small

**Scenario:** ZFS refuses to create pool on 64MB disk images
**Expected Behavior:** `zpool create` command fails with error message
**Implementation:** Requirements explicitly allow increasing disk size if needed (FR4)

**Resolution steps:**
1. Modify `count=64` to `count=128` in dd commands
2. Retry test
3. If still fails, increase to `count=256`

### Edge Case 2: ZFS Kernel Module Not Loaded

**Scenario:** VM boots but ZFS module isn't available
**Expected Behavior:** `zpool create` fails with "command not found" or module error
**Implementation:** `boot.supportedFilesystems = ["zfs"]` ensures module loads

**Debug steps:**
```bash
# In interactive mode:
>>> machine.succeed("lsmod | grep zfs")
>>> machine.succeed("which zpool")
```

### Edge Case 3: Service Startup Delay

**Scenario:** Exporter service takes longer than expected to start
**Expected Behavior:** `wait_for_unit` continues waiting until active or timeout
**Implementation:** Framework handles this automatically, no special handling needed

### Error Scenario 1: Port Already Bound

**Scenario:** Another process is using 127.0.0.1:1234
**Expected Behavior:** Service fails to start, `wait_for_unit` fails test
**Implementation:** VM is isolated, port conflicts are impossible

**Why this can't happen:** Fresh VM has no other services on port 1234

### Error Scenario 2: Pool Created in DEGRADED State

**Scenario:** Mirror pool creation succeeds but pool is DEGRADED (disk issue)
**Expected Behavior:** Precondition validation fails: `grep 'state: ONLINE'` returns no match
**Implementation:** Test fails early with clear indication

**Debug approach:** Inspect full `zpool status` output to see why pool is degraded

### Error Scenario 3: Metrics Show Wrong Values

**Scenario:** Exporter reports scan_state != 40 or scan_age != 876000
**Expected Behavior:** `wait_until_succeeds` times out, test fails
**Implementation:** This indicates a feature regression

**Debug approach:**
```bash
# Inspect full metrics output
>>> machine.succeed("curl http://127.0.0.1:1234/metrics")

# Check exporter logs
>>> machine.succeed("journalctl -u zpool-status-exporter")

# Verify pool state manually
>>> machine.succeed("zpool status newpool")
```

### Error Scenario 4: HELP Text Missing NeverScanned

**Scenario:** Metrics endpoint responds but HELP text doesn't include "NeverScanned = 40"
**Expected Behavior:** Third validation fails
**Implementation:** Indicates `value_enum!` macro or formatting regression

**Root cause:** This would be a code change regression in `src/fmt.rs`

---

## Dependencies

### System Dependencies (All provided by NixOS)

**Required by test:**
- `zfs` - ZFS kernel module and utilities (`zpool`, `zfs` commands)
- `curl` - HTTP client for querying metrics endpoint
- `grep` - Text search for validation
- `dd` - Create file-backed disk images

**Required by framework:**
- `pkgs.nixosTest` - NixOS VM test infrastructure
- Python - Test script environment
- QEMU - VM execution

**All dependencies are standard NixOS tools** - no special packages needed.

### Build Dependencies

**Nix flakes environment:**
- Nix with flakes enabled
- nixosModules.default (zpool-status-exporter module)
- nixpkgs with NixOS test framework

**Inherited from project:**
- `flake.nix` already provides all required infrastructure
- `nix/vm-tests/default.nix` provides test composition
- NixOS module already defined and tested

### Internal Dependencies

**Test depends on:**
- Exporter binary (already built as part of package)
- NixOS module (already defined in `nixosModules.default`)
- ZFS support in NixOS (standard, well-tested)

**Test does NOT depend on:**
- External ZFS pools or disks
- Network access (uses localhost only)
- Persistent storage (everything is ephemeral)

### Constraints

**ZFS constraints:**
- `networking.hostId` must be exactly 8 hexadecimal characters
- `networking.hostId` must be unique per machine (no conflicts in test suite)
- Pool name must be valid ZFS identifier (alphanumeric + underscore)
- Minimum disk size for pool creation (64MB should be sufficient, can increase if needed)

**Test framework constraints:**
- Test name must be valid Nix identifier
- Test script must be valid Python (uses Python 3)
- File-backed pools must use loop devices (ZFS supports this)

---

## Performance Considerations

### Performance Impact: Negligible

**Rationale:**
- Single small pool (2x 64MB = 128MB total)
- No actual data written beyond ZFS metadata
- Metrics queries are fast (< 100ms typically)
- Total test execution time: 10-20 seconds expected

**Breakdown:**
- VM boot: 5-10 seconds
- Service startup: < 1 second
- Disk image creation: < 1 second (64MB × 2)
- Pool creation: < 2 seconds
- Metrics validation: < 1 second (with wait_until_succeeds)

### Resource Usage

**Memory:** ~512MB for VM (standard NixOS test VM)
**Disk:** 128MB for pool images + VM overhead
**CPU:** Minimal (no actual I/O, just metadata operations)

### Scalability

Test execution time is constant and independent of:
- Number of pools on host system (test is isolated)
- Size of real ZFS pools (test uses minimal file-backed pool)
- Number of other tests (tests run in parallel in CI)

---

## Migration & Compatibility

### Backward Compatibility

**This change is 100% backward compatible:**
- Adds new test file only
- Does not modify existing tests
- Does not change exporter behavior
- Does not change NixOS module

**No migration needed:**
- Existing tests continue to work unchanged
- Existing deployments unaffected
- No configuration changes required

### Build System Compatibility

**Nix build targets:**
- Existing: `nix build .#vm-tests` - still works, now includes new test
- New: `nix build .#vm-tests.tests.new-pool-never-scanned` - can build test individually
- New: `nix build .#vm-tests.tests.new-pool-never-scanned.driverInteractive` - interactive debugging

**CI compatibility:**
- `nix build .#ci` - includes all VM tests, will now include new test
- No CI configuration changes needed
- Test runs alongside existing VM tests

---

## Open Questions

**All questions have been resolved:**
- ✅ Pool name: `newpool`
- ✅ Retry mechanism: `wait_until_succeeds`
- ✅ Host ID: `abcd1234`
- ✅ Disk size: Start with 64MB, increase if needed
- ✅ Validation approach: Three separate wait_until_succeeds calls

**No open questions remain.**

---

## Risks & Mitigations

### Risk 1: ZFS Minimum Pool Size

**Description:** ZFS may reject 64MB disk images as too small
**Likelihood:** Low (64MB should be sufficient for ZFS pool metadata)
**Impact:** Medium (test would fail at pool creation step)
**Mitigation:** Requirements explicitly allow increasing to 128MB or 256MB if needed (FR4)

### Risk 2: Timing/Race Conditions

**Description:** Test queries metrics before exporter detects pool
**Likelihood:** Low (using wait_until_succeeds)
**Impact:** Low (would just retry until success)
**Mitigation:** `wait_until_succeeds` automatically retries with 1-second intervals until command succeeds

### Risk 3: Test Takes Too Long

**Description:** wait_until_succeeds could theoretically retry forever
**Likelihood:** Very Low (exporter should detect pool immediately)
**Impact:** Medium (CI timeout, wasted resources)
**Mitigation:**
- Exporter runs `zpool status` on every request (no caching/polling delay)
- Precondition validation ensures pool exists before metrics validation
- Can manually test timeout behavior in interactive mode

### Risk 4: Flaky Test

**Description:** Test passes sometimes, fails other times due to environmental variations
**Likelihood:** Very Low (test is deterministic, VM is isolated)
**Impact:** High (CI noise, false failures)
**Mitigation:**
- VM is fully isolated (no external dependencies)
- File-backed pools are deterministic
- wait_until_succeeds handles timing variations
- Existing VM tests are stable, same pattern here

---

## Acceptance Criteria

Implementation will be considered complete when:

### File Creation and Structure
- [x] File `nix/vm-tests/new-pool-never-scanned.nix` exists
- [x] File follows structure of existing VM tests
- [x] File accepts `pkgs` and `nixosModule` parameters
- [x] File defines `listen_address` and `poolname` bindings
- [x] Test name is "new-pool-never-scanned"

### Machine Configuration
- [x] ZFS filesystem support enabled (`boot.supportedFilesystems = ["zfs"]`)
- [x] Unique `networking.hostId = "abcd1234"` (8 hex chars, different from other tests)
- [x] `zpool-status-exporter` service is enabled
- [x] Service is configured with correct `listen_address`

### Test Script - Service Startup
- [x] Test waits for `default.target` before proceeding
- [x] Test waits for `zpool-status-exporter.service` before proceeding

### Test Script - Pool Creation
- [x] Two disk images created using `dd` command (64MB each)
- [x] Mirror pool created with name "newpool"
- [x] Pool creation commands use `machine.succeed()` assertions

### Test Script - Precondition Validation
- [x] Test verifies pool state is ONLINE
- [x] Test verifies no scan line exists (using `machine.fail()` for grep)

### Test Script - Metrics Validation
- [x] Test uses `wait_until_succeeds()` for metrics validation
- [x] Test validates `zpool_scan_state{pool="newpool"} 40` (exact match)
- [x] Test validates `zpool_scan_age{pool="newpool"} 876000` (exact match)
- [x] Test validates HELP text contains "NeverScanned = 40"

### Integration
- [x] Test added to `nix/vm-tests/default.nix` in `test_sources`
- [x] Test can be run via `nix build .#vm-tests` (all tests)
- [x] Test can be run individually via `nix build .#vm-tests.tests.new-pool-never-scanned`
- [x] Test passes successfully

### Code Quality
- [x] Nix code is formatted with `alejandra`
- [x] Code follows conventions of existing VM tests
- [x] No Nix syntax errors or warnings

### Documentation
- [x] Code includes comments explaining key steps
- [x] Pool name and configuration are clearly defined
- [x] wait_until_succeeds usage is documented

---

## Appendix A: Complete Test File Template

```nix
{
  pkgs,
  nixosModule,
}: let
  listen_address = "127.0.0.1:1234";
  poolname = "newpool";
in
  pkgs.nixosTest {
    name = "new-pool-never-scanned";

    nodes.machine = {pkgs, ...}: {
      imports = [nixosModule];
      boot.supportedFilesystems = ["zfs"];
      networking.hostId = "abcd1234";
      services.zpool-status-exporter = {
        enable = true;
        inherit listen_address;
      };
    };

    testScript = ''
      # Wait for system and service to be ready
      machine.wait_for_unit("default.target")
      machine.wait_for_unit("zpool-status-exporter.service")

      # Create file-backed disk images (64MB each)
      machine.succeed("dd if=/dev/zero of=/tmp/disk1.img bs=1M count=64")
      machine.succeed("dd if=/dev/zero of=/tmp/disk2.img bs=1M count=64")

      # Create mirror pool (new pool will have no scan line)
      machine.succeed("zpool create ${poolname} mirror /tmp/disk1.img /tmp/disk2.img")

      # Verify preconditions: pool is ONLINE and has never been scanned
      machine.succeed("zpool status ${poolname} | grep 'state: ONLINE'")
      machine.fail("zpool status ${poolname} | grep 'scan:'")

      # Validate metrics (wait_until_succeeds handles timing of pool detection)
      machine.wait_until_succeeds("curl http://${listen_address}/metrics | grep 'zpool_scan_state{pool=\"${poolname}\"} 40'")
      machine.wait_until_succeeds("curl http://${listen_address}/metrics | grep 'zpool_scan_age{pool=\"${poolname}\"} 876000'")
      machine.wait_until_succeeds("curl http://${listen_address}/metrics | grep 'NeverScanned = 40'")
    '';
  }
```

---

## Appendix B: default.nix Integration

**File:** `nix/vm-tests/default.nix`

**Change required (lines 6-10):**

```nix
test_sources = {
  empty-zfs = ./empty-zfs.nix;
  empty-zfs-auth = ./empty-zfs-auth.nix;
  max-bind-retries = ./max-bind-retries.nix;
  new-pool-never-scanned = ./new-pool-never-scanned.nix;  # ADD THIS LINE
};
```

**Rationale:** Adds test to the suite while maintaining alphabetical ordering.

---

## Appendix C: Running and Debugging the Test

### Run all VM tests
```bash
nix build .#vm-tests
```

### Run only the new test
```bash
nix build .#vm-tests.tests.new-pool-never-scanned
```

### Run interactively for debugging
```bash
nix build .#vm-tests.tests.new-pool-never-scanned.driverInteractive
./result/bin/nixos-test-driver
```

### Interactive debugging commands
```python
# Start the VM
>>> start_all()

# Run individual commands manually
>>> machine.succeed("zpool list")
>>> machine.succeed("zpool status")
>>> machine.succeed("curl http://127.0.0.1:1234/metrics")

# Check service logs
>>> machine.succeed("journalctl -u zpool-status-exporter")

# Check ZFS status
>>> machine.succeed("zpool status newpool")

# Full metrics output
>>> machine.succeed("curl -s http://127.0.0.1:1234/metrics | grep -A5 scan")
```

---

## Appendix D: Expected Metrics Output

When the test passes, the `/metrics` endpoint should return (excerpt):

```prometheus
# HELP zpool_scan_state Scan status: UnknownMissing = 0, Unrecognized = 1, ScrubRepaired = 10, Resilvered = 15, ScrubInProgress = 30, ScrubCanceled = 35, NeverScanned = 40
# TYPE zpool_scan_state gauge
zpool_scan_state{pool="newpool"} 40

# HELP zpool_scan_age Scan age in hours
# TYPE zpool_scan_age gauge
zpool_scan_age{pool="newpool"} 876000
```

**Key validations:**
- ✅ HELP text includes "NeverScanned = 40"
- ✅ scan_state for "newpool" is exactly 40
- ✅ scan_age for "newpool" is exactly 876000 (100 years in hours)

---

## Summary

This specification provides a complete, detailed plan for implementing a NixOS VM test that validates the NeverScanned feature end-to-end. The implementation:

- **Follows existing patterns** from empty-zfs.nix and empty-zfs-auth.nix
- **Uses robust retry mechanism** (wait_until_succeeds) for reliable testing
- **Validates all requirements** from REQUIREMENTS.md (FR1-FR8)
- **Requires no code changes** - only adds a new test file and test suite integration
- **Provides debugging support** via interactive mode
- **Integrates seamlessly** with existing build and CI infrastructure

**Implementation complexity:** Low - single test file + one-line integration change
**Test execution time:** 10-20 seconds expected
**Maintenance burden:** Minimal - follows established patterns

The specification is complete and ready for implementation without requiring architectural decisions from the implementer. All design choices have been made and justified.
