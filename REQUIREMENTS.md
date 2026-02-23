# Requirements: NixOS VM Test for NeverScanned Pool Status

## 1. Overview

### Problem Statement
The "pool-never-scanned" feature (PR #01) was implemented and merged, but Phase 6 (NixOS VM Test) was marked as optional and not completed. The feature has comprehensive Rust integration tests but lacks end-to-end validation in a real NixOS environment with actual ZFS pools.

### Motivation
While Rust integration tests verify the parsing logic using fixture data, a VM test provides:
- **Real ZFS validation**: Tests actual `zpool status` output from the ZFS kernel module, not mocked data
- **Full-stack integration**: Validates the complete deployment stack (NixOS module, systemd service, kernel module, HTTP endpoint)
- **Production confidence**: Demonstrates the feature works in a production-like environment
- **Regression prevention**: Catches issues that might arise from NixOS module changes, ZFS version updates, or systemd configuration changes

### High-Level Goals
1. Create a NixOS VM test that validates the NeverScanned feature works end-to-end
2. Follow existing VM test patterns and conventions
3. Test only the core functionality (new pool detection) without state transitions
4. Provide strict validation of the critical metrics

## 2. Functional Requirements

### FR1: Test File Creation
Create a new VM test file at `nix/vm-tests/new-pool-never-scanned.nix`.

**File structure:**
- Follow the pattern established by `empty-zfs.nix` and `empty-zfs-auth.nix`
- Accept `pkgs` and `nixosModule` parameters
- Define `listen_address` as a local binding
- Return a `pkgs.nixosTest` structure

**Example skeleton:**
```nix
{
  pkgs,
  nixosModule,
}: let
  listen_address = "127.0.0.1:1234";
in
  pkgs.nixosTest {
    name = "new-pool-never-scanned";
    nodes.machine = { ... };
    testScript = '' ... '';
  }
```

### FR2: NixOS Machine Configuration
The test VM must be configured with:
- ZFS filesystem support enabled
- A unique networking.hostId (required by ZFS)
- The zpool-status-exporter service enabled
- The service configured to listen on the specified address

**Configuration requirements:**
```nix
nodes.machine = {pkgs, ...}: {
  imports = [nixosModule];
  boot.supportedFilesystems = ["zfs"];
  networking.hostId = "XXXXXXXX";  # 8 hex characters, unique from other tests
  services.zpool-status-exporter = {
    enable = true;
    inherit listen_address;
  };
};
```

**Host ID constraint**: Must be exactly 8 hexadecimal characters, different from existing tests:
- `empty-zfs.nix`: `039419bd`
- `empty-zfs-auth.nix`: `139419bd`
- Choose a new, unique value

### FR3: Test Script - Service Startup
The test must wait for the system and service to be ready before proceeding.

**Required steps:**
1. Wait for `default.target` to be active
2. Wait for `zpool-status-exporter.service` to be active

**Implementation:**
```nix
testScript = ''
  machine.wait_for_unit("default.target")
  machine.wait_for_unit("zpool-status-exporter.service")
  # ... additional steps ...
'';
```

### FR4: Test Script - Pool Creation
Create a file-backed mirror pool for testing.

**Steps:**
1. Create two file-backed disk images using `dd`
2. Create a mirror pool using `zpool create`
3. Use a descriptive pool name

**Requirements:**
- **Pool name**: Use a descriptive name (e.g., `testpool`, `newpool`, or `never_scanned_pool`)
- **Mirror configuration**: Two disk images in a mirror
- **Initial disk size**: 64MB per disk (`bs=1M count=64`)
- **Flexibility**: If 64MB fails during implementation, the implementer may increase the size as needed
- **File location**: Use `/tmp/` for disk images (ephemeral, VM-local)

**Example implementation:**
```bash
machine.succeed("dd if=/dev/zero of=/tmp/disk1.img bs=1M count=64")
machine.succeed("dd if=/dev/zero of=/tmp/disk2.img bs=1M count=64")
machine.succeed("zpool create <poolname> mirror /tmp/disk1.img /tmp/disk2.img")
```

### FR5: Test Script - Precondition Validation
Before checking metrics, verify the pool is in the expected state.

**Validation checks:**
1. **Pool is ONLINE**: Verify `zpool status` shows `state: ONLINE`
2. **No scan line present**: Verify `zpool status` does NOT contain a `scan:` line

**Implementation:**
```bash
# Verify pool is ONLINE
machine.succeed("zpool status <poolname> | grep 'state: ONLINE'")

# Verify no scan line exists (grep should fail)
machine.fail("zpool status <poolname> | grep 'scan:'")
```

**Rationale**: These checks confirm the pool meets the criteria for NeverScanned detection (ONLINE state, no scan line) before testing the exporter metrics.

### FR6: Test Script - Metrics Validation with Retry
Query the metrics endpoint and validate the NeverScanned feature is working.

**Wait/Retry Logic:**
After creating the pool, the exporter may need time to detect and process it. The test should:
- Use retry logic or a small delay to allow the exporter to detect the new pool
- Not fail immediately if the metric isn't present on first request

**Validation requirements (strict):**

1. **Scan State Metric**: `zpool_scan_state{pool="<poolname>"} 40`
   - Exact match required (value must be exactly 40)
   - Pool label must match the created pool name

2. **Scan Age Metric**: `zpool_scan_age{pool="<poolname>"} 876000`
   - Exact match required (value must be exactly 876000)
   - Pool label must match the created pool name

3. **HELP Text**: Must include "NeverScanned = 40"
   - Verify the HELP text for `zpool_scan_state` documents the new status value
   - Exact substring match required

**Implementation approach:**
```bash
# Add delay/retry logic to wait for exporter to process the new pool
# (Implementation details left to implementer - could be sleep, retry loop, or wait_until_succeeds)

# Validate scan_state metric
machine.succeed("curl http://${listen_address}/metrics | grep 'zpool_scan_state{pool=\"<poolname>\"} 40'")

# Validate scan_age metric
machine.succeed("curl http://${listen_address}/metrics | grep 'zpool_scan_age{pool=\"<poolname>\"} 876000'")

# Validate HELP text
machine.succeed("curl http://${listen_address}/metrics | grep 'NeverScanned = 40'")
```

**Note on retry logic**: The implementer should choose an appropriate retry mechanism. Options include:
- `machine.wait_until_succeeds("...")` (if available in nixosTest)
- Simple `sleep N` delay before validation
- Retry loop with timeout

### FR7: Test Script - No Cleanup Required
The test does not need to destroy the pool at the end.

**Rationale**: The VM is ephemeral and will be destroyed after the test completes. Explicit cleanup adds no value and increases test complexity.

### FR8: Integration with Test Suite
Add the new test to the test suite configuration.

**File to modify**: `nix/vm-tests/default.nix`

**Change required**: Add `new-pool-never-scanned` to `test_sources` attribute set:
```nix
test_sources = {
  empty-zfs = ./empty-zfs.nix;
  empty-zfs-auth = ./empty-zfs-auth.nix;
  max-bind-retries = ./max-bind-retries.nix;
  new-pool-never-scanned = ./new-pool-never-scanned.nix;  # NEW
};
```

**Location**: Between lines 6-10 in the existing file structure.

## 3. Non-Functional Requirements

### NFR1: Test Reliability
- The test must be deterministic and reproducible
- Must not have race conditions or timing dependencies (use proper retry/wait logic)
- Must fail clearly if the feature is broken (no false positives)

### NFR2: Test Performance
- Should complete in reasonable time (< 2 minutes)
- Pool creation and metrics validation should not require excessive disk space or memory
- If 64MB disk images prove insufficient, increasing to 128MB or 256MB is acceptable

### NFR3: Code Quality
- Follow Nix formatting conventions (use `alejandra` formatter)
- Match the style and structure of existing VM tests
- Clear, readable test script with logical step progression

### NFR4: Maintainability
- Test should be self-contained and understandable
- Comments should explain non-obvious steps (e.g., why we wait/retry)
- No hard-coded values that might break with future changes (except the specific metric values we're testing)

### NFR5: Integration
- Must work with existing Nix build infrastructure
- Must be runnable via `nix build .#vm-tests` (all tests)
- Must be runnable individually via `nix build .#vm-tests.tests.new-pool-never-scanned`
- Must support interactive debugging via `.driverInteractive`

## 4. Scope

### Explicitly In Scope
✅ Creating a new VM test file (`new-pool-never-scanned.nix`)
✅ Testing new pool detection (NeverScanned = 40)
✅ Validating scan age metric (876000 hours)
✅ Validating HELP text includes NeverScanned
✅ Precondition validation (ONLINE state, no scan line)
✅ File-backed mirror pool creation
✅ Retry/wait logic for metrics detection
✅ Integration with existing test suite (`default.nix` update)

### Explicitly Out of Scope
❌ Testing degraded pools without scan lines (covered by Rust tests)
❌ Testing pool state transitions (NeverScanned → ScrubInProgress → ScrubRepaired)
❌ Testing authentication (covered by `empty-zfs-auth.nix`)
❌ Testing service binding/restart behavior (covered by `max-bind-retries.nix`)
❌ Testing multiple pools in one test
❌ Testing different pool configurations (raidz, single disk, etc.)
❌ Modifying the exporter code itself
❌ Adding new metrics or changing metric values

### Future Considerations
- Could add VM tests for other scan states if needed
- Could add tests for pool state transitions (scrub lifecycle)
- Could add tests for multi-pool scenarios

## 5. User Stories / Use Cases

### US1: Validate NeverScanned Feature End-to-End
**As a** project maintainer
**I want** a VM test that validates the NeverScanned feature in a real NixOS environment
**So that** I can be confident the feature works in production deployments

**Scenario:**
1. NixOS VM boots with ZFS support and zpool-status-exporter service
2. A new mirror pool is created using file-backed disks
3. The pool is verified to be ONLINE with no scan line
4. The metrics endpoint is queried
5. The metrics show `zpool_scan_state` = 40 (NeverScanned)
6. The metrics show `zpool_scan_age` = 876000 (100 years)
7. The HELP text documents "NeverScanned = 40"
8. Test passes, confirming end-to-end functionality

### US2: Regression Prevention for Future Changes
**As a** contributor making changes to the NixOS module or exporter
**I want** automated VM tests to catch regressions
**So that** I don't accidentally break the NeverScanned feature

**Scenario:**
1. Developer modifies the NixOS module configuration
2. Developer runs `nix build .#vm-tests` to run all VM tests
3. If the change breaks NeverScanned detection, the `new-pool-never-scanned` test fails
4. Developer identifies and fixes the regression before merging

### US3: Manual Testing and Debugging
**As a** developer debugging a NeverScanned issue
**I want** to run the VM test interactively
**So that** I can inspect the system state and diagnose problems

**Scenario:**
1. Developer runs `nix build .#vm-tests.tests.new-pool-never-scanned.driverInteractive`
2. Developer runs `./result/bin/nixos-test-driver` to start interactive mode
3. Developer can manually execute test steps and inspect outputs
4. Developer can run `zpool status` and `curl` commands to debug
5. Developer identifies the root cause and fixes the issue

## 6. Acceptance Criteria

### AC1: File Creation and Structure
- [ ] File `nix/vm-tests/new-pool-never-scanned.nix` exists
- [ ] File follows the structure of existing VM tests
- [ ] File accepts `pkgs` and `nixosModule` parameters
- [ ] File defines `listen_address` binding
- [ ] Test name is "new-pool-never-scanned"

### AC2: Machine Configuration
- [ ] ZFS filesystem support enabled (`boot.supportedFilesystems = ["zfs"]`)
- [ ] Unique `networking.hostId` is set (8 hex chars, different from other tests)
- [ ] `zpool-status-exporter` service is enabled
- [ ] Service is configured with correct `listen_address`

### AC3: Test Script - Service Startup
- [ ] Test waits for `default.target` before proceeding
- [ ] Test waits for `zpool-status-exporter.service` before proceeding

### AC4: Test Script - Pool Creation
- [ ] Two disk images are created using `dd` command
- [ ] Disk images are 64MB each (or larger if needed)
- [ ] Mirror pool is created with descriptive name
- [ ] Pool creation commands use `machine.succeed()` assertions

### AC5: Test Script - Precondition Validation
- [ ] Test verifies pool state is ONLINE
- [ ] Test verifies no scan line exists (using `machine.fail()` for grep)

### AC6: Test Script - Metrics Validation
- [ ] Test includes retry/wait logic before metrics validation
- [ ] Test validates `zpool_scan_state{pool="..."} 40` (exact match)
- [ ] Test validates `zpool_scan_age{pool="..."} 876000` (exact match)
- [ ] Test validates HELP text contains "NeverScanned = 40"
- [ ] All validations use `machine.succeed()` with curl + grep

### AC7: Integration
- [ ] Test is added to `nix/vm-tests/default.nix` in `test_sources`
- [ ] Test can be run via `nix build .#vm-tests` (all tests)
- [ ] Test can be run individually via `nix build .#vm-tests.tests.new-pool-never-scanned`
- [ ] Test passes successfully

### AC8: Code Quality
- [ ] Nix code is formatted with `alejandra`
- [ ] Code follows conventions of existing VM tests
- [ ] No Nix syntax errors or warnings

### AC9: Documentation
- [ ] Code includes comments explaining non-obvious steps
- [ ] Pool name and configuration are clearly defined
- [ ] Retry/wait logic is documented (if non-trivial)

## 7. Edge Cases and Error Handling

### EC1: Pool Creation Fails
**Scenario**: `zpool create` command fails (e.g., disk images too small, ZFS module not loaded)

**Expected Behavior**: Test fails with clear error message from `machine.succeed()`

**Handling**: The `machine.succeed()` wrapper will fail the test and show the command output, making debugging straightforward.

### EC2: Service Not Ready When Pool Created
**Scenario**: Pool is created before the exporter service has fully started

**Expected Behavior**: Initial metrics queries might fail or return incomplete data

**Handling**: FR6 requires retry/wait logic to handle this case. The test should wait for the service to be ready and detect the pool.

### EC3: Metrics Endpoint Returns 500 Error
**Scenario**: Exporter encounters an error processing the pool

**Expected Behavior**: Test fails when curl + grep doesn't find expected metrics

**Handling**: The `machine.succeed()` wrapper fails the test. Developer investigates using interactive mode.

### EC4: Pool is Created But Not ONLINE
**Scenario**: Pool creation succeeds but pool enters DEGRADED or other state

**Expected Behavior**: Test fails at precondition validation step (grep for "state: ONLINE" fails)

**Handling**: Test fails early with clear indication that the pool state is wrong. Developer can debug why the pool isn't ONLINE.

### EC5: Exporter Takes Too Long to Detect Pool
**Scenario**: Even with retry logic, the exporter doesn't detect the pool within the timeout

**Expected Behavior**: Test fails at metrics validation step

**Handling**: Developer should investigate:
- Is the exporter running? (service check would have caught this)
- Is the exporter polling interval too long?
- Is there a bug in pool detection?

Using interactive mode (`driverInteractive`) allows manual inspection.

### EC6: Disk Size Too Small (64MB Insufficient)
**Scenario**: ZFS refuses to create a pool on 64MB disk images

**Expected Behavior**: Pool creation fails with ZFS error message

**Handling**: FR4 explicitly allows the implementer to increase disk size if needed. Change the `count=64` to `count=128` or `count=256` and retry.

### EC7: HELP Text Missing NeverScanned
**Scenario**: Feature regression causes HELP text to not include "NeverScanned = 40"

**Expected Behavior**: Test fails at HELP text validation step

**Handling**: Clear test failure indicates the `value_enum!` macro or formatting code has regressed. This is exactly the kind of regression the test is meant to catch.

## 8. Dependencies and Constraints

### System Dependencies
- **NixOS**: Test runs in NixOS VM environment (provided by `pkgs.nixosTest`)
- **ZFS Kernel Module**: Must be available and loadable in the test VM
- **curl**: Must be available in the test VM for HTTP requests
- **grep**: Must be available in the test VM for output validation
- **dd**: Must be available in the test VM for disk image creation

All of these are standard NixOS tools and should be available by default.

### Build Dependencies
- **Nix with flakes**: Required to run `nix build .#vm-tests`
- **nixosModule**: The zpool-status-exporter NixOS module must be available
- **VM testing infrastructure**: NixOS test framework (`pkgs.nixosTest`)

### Constraints from Existing Code
- Must follow the pattern established by existing VM tests
- Must use the same `listen_address` pattern as other tests (`127.0.0.1:1234`)
- Must integrate with the existing `default.nix` test suite structure

### ZFS Constraints
- Requires unique `networking.hostId` (8 hex characters)
- Minimum pool size requirements (64MB may be too small, implementer can adjust)
- ZFS pools created in `/tmp/` (VM-local, ephemeral)

### Performance Constraints
- VM test execution time should be reasonable (< 2 minutes)
- Disk image creation should not require excessive space (64-256MB total)
- Retry/wait logic should have reasonable timeout (suggest 10-30 seconds max)

## 9. Open Questions

**None** - all questions have been resolved through the requirements interview.

**Previously resolved:**
- [x] Scope: Only new-pool test, not degraded-pool test
- [x] Test depth: Basic functionality only, no state transitions
- [x] Configuration: File-backed mirror pool
- [x] Integration points: Only test exporter metrics
- [x] Validation: Strict validation for all three items (scan_state, scan_age, HELP text)
- [x] Pool name: Descriptive name (e.g., testpool, newpool)
- [x] Precondition checks: Verify ONLINE state and no scan line
- [x] Disk size: Start with 64MB, increase if needed
- [x] Cleanup: No cleanup needed (VM is ephemeral)
- [x] Service refresh: Use retry/wait logic for robust detection
- [x] Test name: `new-pool-never-scanned.nix`

## 10. Appendices

### Appendix A: Complete Test Structure Template

```nix
{
  pkgs,
  nixosModule,
}: let
  listen_address = "127.0.0.1:1234";
  poolname = "newpool";  # or "testpool", implementer's choice
in
  pkgs.nixosTest {
    name = "new-pool-never-scanned";

    nodes.machine = {pkgs, ...}: {
      imports = [nixosModule];
      boot.supportedFilesystems = ["zfs"];
      networking.hostId = "XXXXXXXX";  # Choose unique 8 hex chars
      services.zpool-status-exporter = {
        enable = true;
        inherit listen_address;
      };
    };

    testScript = ''
      # 1. Wait for system and service to be ready
      machine.wait_for_unit("default.target")
      machine.wait_for_unit("zpool-status-exporter.service")

      # 2. Create file-backed disk images
      machine.succeed("dd if=/dev/zero of=/tmp/disk1.img bs=1M count=64")
      machine.succeed("dd if=/dev/zero of=/tmp/disk2.img bs=1M count=64")

      # 3. Create mirror pool
      machine.succeed("zpool create ${poolname} mirror /tmp/disk1.img /tmp/disk2.img")

      # 4. Verify preconditions
      machine.succeed("zpool status ${poolname} | grep 'state: ONLINE'")
      machine.fail("zpool status ${poolname} | grep 'scan:'")

      # 5. Wait/retry for exporter to detect the pool
      # (implementer chooses retry mechanism - sleep, wait_until_succeeds, retry loop, etc.)

      # 6. Validate metrics
      machine.succeed("curl http://${listen_address}/metrics | grep 'zpool_scan_state{pool=\"${poolname}\"} 40'")
      machine.succeed("curl http://${listen_address}/metrics | grep 'zpool_scan_age{pool=\"${poolname}\"} 876000'")
      machine.succeed("curl http://${listen_address}/metrics | grep 'NeverScanned = 40'")
    '';
  }
```

### Appendix B: Retry Logic Options

**Option 1: Simple sleep**
```bash
machine.succeed("sleep 5")  # Wait 5 seconds for exporter to process pool
```

**Option 2: wait_until_succeeds (if available)**
```bash
machine.wait_until_succeeds("curl http://${listen_address}/metrics | grep 'zpool_scan_state{pool=\"${poolname}\"} 40'")
```

**Option 3: Manual retry loop**
```python
# Note: nixosTest testScript supports Python
for i in range(10):
    try:
        machine.succeed("curl http://${listen_address}/metrics | grep 'zpool_scan_state{pool=\"${poolname}\"} 40'")
        break
    except:
        if i == 9:
            raise
        machine.succeed("sleep 2")
```

The implementer should choose the most appropriate option based on NixOS test framework capabilities.

### Appendix C: Expected Test Output

When the test passes, you should see output similar to:

```
machine: waiting for unit default.target
machine: unit default.target has reached state active
machine: waiting for unit zpool-status-exporter.service
machine: unit zpool-status-exporter.service has reached state active
machine: must succeed: dd if=/dev/zero of=/tmp/disk1.img bs=1M count=64
machine: must succeed: dd if=/dev/zero of=/tmp/disk2.img bs=1M count=64
machine: must succeed: zpool create newpool mirror /tmp/disk1.img /tmp/disk2.img
machine: must succeed: zpool status newpool | grep 'state: ONLINE'
machine: must fail: zpool status newpool | grep 'scan:'
machine: must succeed: curl http://127.0.0.1:1234/metrics | grep 'zpool_scan_state{pool="newpool"} 40'
machine: must succeed: curl http://127.0.0.1:1234/metrics | grep 'zpool_scan_age{pool="newpool"} 876000'
machine: must succeed: curl http://127.0.0.1:1234/metrics | grep 'NeverScanned = 40'
test script finished in X.XXs
```

### Appendix D: Running the Test

**Run all VM tests:**
```bash
nix build .#vm-tests
```

**Run only the new-pool-never-scanned test:**
```bash
nix build .#vm-tests.tests.new-pool-never-scanned
```

**Run interactively for debugging:**
```bash
nix build .#vm-tests.tests.new-pool-never-scanned.driverInteractive
./result/bin/nixos-test-driver
```

In interactive mode, you can run commands manually:
```python
>>> start_all()
>>> machine.succeed("zpool list")
>>> machine.succeed("curl http://127.0.0.1:1234/metrics")
>>> # etc.
```

### Appendix E: Integration with CI

Once implemented, this test will run as part of:
```bash
nix build .#ci
```

This ensures the NeverScanned feature is validated in every CI run, providing continuous regression protection.

### Appendix F: Troubleshooting Guide

**Test fails at pool creation:**
- Check if ZFS kernel module loaded: `machine.succeed("lsmod | grep zfs")`
- Try larger disk images: increase `count=64` to `count=128` or `count=256`
- Check ZFS minimum pool size requirements

**Test fails at metrics validation:**
- Verify service is running: `machine.succeed("systemctl status zpool-status-exporter")`
- Check service logs: `machine.succeed("journalctl -u zpool-status-exporter")`
- Manually query metrics: `machine.succeed("curl http://127.0.0.1:1234/metrics")`
- Verify pool exists: `machine.succeed("zpool list")`
- Check pool status: `machine.succeed("zpool status")`

**Test times out:**
- Increase retry timeout
- Check if exporter polling interval is too long
- Verify no deadlocks or hangs in exporter

**Interactive debugging:**
Use `.driverInteractive` to step through the test manually and inspect system state at each stage.
