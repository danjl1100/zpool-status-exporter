# Implementing Max Bind Retries: Implementation Guide

This document outlines how to implement robust network binding with retry logic, based on lessons learned from the kopia-exporter implementation.

## Problem Statement

Network services that bind to specific interfaces (especially Tailscale interfaces) can fail during:
- System startup when interfaces aren't ready yet
- Resume from sleep when network stack is reinitializing  
- Interface reconfiguration or network changes

The default behavior is immediate failure, requiring external restart mechanisms.

## Solution Overview

Implement application-level exponential backoff retry logic that:
1. Makes an initial binding attempt
2. On failure, retries with increasing delays
3. Provides clear logging for debugging
4. Follows standard CLI semantics

## Implementation Details

### 1. CLI Interface Design

**Parameter Semantics:**
- `--max-bind-retries N` means N retry attempts after the initial attempt
- `--max-bind-retries 0` = 1 attempt only (no retries)
- `--max-bind-retries 5` = 6 total attempts (1 initial + 5 retries)

**Help Text:**
```
--max-bind-retries <N>    Maximum number of bind retry attempts (0 = no retries, just 1 attempt) [default: 5]
```

### 2. Retry Logic Implementation

**Core Algorithm:**
```rust
fn calculate_delay_seconds(attempt: u32) -> u64 {
    (1u64 << (attempt - 1)).min(16) // 1, 2, 4, 8, 16, 16, 16... seconds (capped at 16)
}

fn start_server_with_retry(bind_addr: &str, max_retries: u32) -> Result<Server> {
    let mut attempt = 1;
    let mut retries_remaining = max_retries;
    
    loop {
        // 1. Attempt connection
        match bind_to_address(bind_addr) {
            Ok(server) => {
                if attempt > 1 {
                    println!("Successfully bound to {bind_addr} on attempt {attempt}");
                }
                return Ok(server);
            }
            Err(e) => {
                // 2. Check retries remaining
                if retries_remaining == 0 {
                    // 4. Return error when exhausted
                    return Err(format!("Failed to bind to {bind_addr} after {attempt} attempts: {e}"));
                }
                
                // 3. Delay and continue if retries available
                let delay_secs = calculate_delay_seconds(attempt);
                eprintln!("Bind attempt {attempt} failed: {e}. Retrying in {delay_secs}s...");
                sleep(Duration::from_secs(delay_secs));
                
                attempt += 1;
                retries_remaining -= 1;
            }
        }
    }
}
```

**Key Insights:**
- Use exponential backoff: `1 << (attempt - 1)` gives 1, 2, 4, 8, 16 second delays
- Log each attempt for debugging network issues
- Show success message only on retry attempts (not first success)
- Natural loop flow is clearer than counting iterations

### 3. Integration Points

**Main Function Integration:**
```rust
fn main() -> Result<()> {
    let args = parse_args();
    
    // Replace direct binding with retry logic
    let server = start_server_with_retry(&args.bind, args.max_bind_retries)?;
    
    // Continue with normal server operation
    serve_requests(server);
    Ok(())
}
```

**Error Handling:**
- Preserve original error message in final failure
- Use structured error types if available
- Ensure errors are actionable for users

### 4. Testing Strategy

**Unit Tests:**
```rust
#[test]
fn test_no_retries_single_attempt() {
    // Test that 0 retries = exactly 1 attempt
    let result = start_server_with_retry("127.0.0.1:99999", 0);
    assert!(result.is_err());
    assert!(error_message.contains("after 1 attempts"));
}

#[test]  
fn test_retry_exhaustion() {
    // Test that N retries = N+1 total attempts
    let result = start_server_with_retry(occupied_port, 2);
    assert!(error_message.contains("after 3 attempts"));
}

#[test]
fn test_success_after_port_freed() {
    // Test recovery when port becomes available
    // 1. Occupy port
    // 2. Start retry process in background  
    // 3. Free port after delay
    // 4. Verify eventual success
}
```

**Integration Tests:**
```rust
#[test]
fn test_cli_flag_help() {
    let output = run_binary(&["--help"]);
    assert!(output.contains("--max-bind-retries"));
    assert!(output.contains("Maximum number of bind retry attempts"));
}

#[test]
fn test_retry_behavior_with_real_binary() {
    // Use env!("CARGO_BIN_EXE_<binary>") for direct binary execution
    // Test actual retry timing and logging output
}
```

### 5. Configuration System Integration

**For Systems with Config Files:**
```yaml
server:
  bind: "127.0.0.1:8080"
  max_bind_retries: 5
```

**For Systemd Services:**
```ini
[Service]
ExecStart=/usr/bin/myservice --bind 100.64.1.2:8080 --max-bind-retries 10
```

**For NixOS Modules:**
```nix
options.services.myservice = {
  maxBindRetries = mkOption {
    type = types.ints.unsigned;
    default = 5;
    description = "Maximum number of bind retry attempts (0 = no retries, just 1 attempt).";
  };
};
```

### 6. Language-Specific Adaptations

**Go Implementation:**
```go
func startServerWithRetry(addr string, maxRetries int) (*http.Server, error) {
    attempt := 1
    retriesRemaining := maxRetries
    
    for {
        listener, err := net.Listen("tcp", addr)
        if err == nil {
            if attempt > 1 {
                log.Printf("Successfully bound to %s on attempt %d", addr, attempt)
            }
            return &http.Server{}, nil
        }
        
        if retriesRemaining == 0 {
            return nil, fmt.Errorf("failed to bind to %s after %d attempts: %v", addr, attempt, err)
        }
        
        delaySecs := 1 << (attempt - 1)
        log.Printf("Bind attempt %d failed: %v. Retrying in %ds...", attempt, err, delaySecs)
        time.Sleep(time.Duration(delaySecs) * time.Second)
        
        attempt++
        retriesRemaining--
    }
}
```

**Python Implementation:**
```python
import time
import socket

def start_server_with_retry(bind_addr, max_retries):
    attempt = 1
    retries_remaining = max_retries
    
    while True:
        try:
            # Attempt to bind
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.bind(bind_addr)
            if attempt > 1:
                print(f"Successfully bound to {bind_addr} on attempt {attempt}")
            return sock
        except OSError as e:
            if retries_remaining == 0:
                raise Exception(f"Failed to bind to {bind_addr} after {attempt} attempts: {e}")
            
            delay_secs = 1 << (attempt - 1)
            print(f"Bind attempt {attempt} failed: {e}. Retrying in {delay_secs}s...")
            time.sleep(delay_secs)
            
            attempt += 1
            retries_remaining -= 1
```

### 7. Common Pitfalls to Avoid

**CLI Semantics:**
- ❌ Don't make `--max-retries 0` mean "infinite retries"
- ❌ Don't count total attempts instead of retry attempts
- ✅ Follow convention: parameter = additional attempts after initial

**Timing:**
- ❌ Don't use fixed delays (overwhelms failing services)
- ❌ Don't use random jitter (makes debugging harder)
- ✅ Use exponential backoff with reasonable caps (16s max)

**Logging:**
- ❌ Don't log success on first attempt (creates noise)
- ❌ Don't log retries at DEBUG level (users need to see them)
- ✅ Log attempts at INFO/WARN level for visibility

**Testing:**
- ❌ Don't use `cargo run` in integration tests (slow)
- ❌ Don't assume fixed port numbers (causes test conflicts)  
- ✅ Use `env!("CARGO_BIN_EXE_*")` and port 0 for dynamic allocation

### 8. Deployment Considerations

**Systemd Integration:**
Users may want both application-level retries AND systemd dependencies:
```ini
[Unit]
After=tailscaled.service network.target
BindsTo=tailscaled.service

[Service]  
ExecStart=/usr/bin/myservice --max-bind-retries 10
```

**Monitoring:**
- Expose retry attempt metrics if using Prometheus
- Log structured data for parsing by monitoring systems
- Consider alerting on repeated bind failures

**Documentation:**
- Include Tailscale-specific setup examples
- Document interaction with firewalls and SELinux
- Provide troubleshooting guide for common binding issues

## Example Implementation Checklist

- [ ] Add CLI flag with proper semantics and help text
- [ ] Implement retry logic with exponential backoff  
- [ ] Replace direct binding calls with retry function
- [ ] Add unit tests for retry behavior
- [ ] Add integration tests with real binary
- [ ] Update configuration system (if applicable)
- [ ] Update systemd/NixOS modules (if applicable)
- [ ] Test with actual Tailscale interface scenarios
- [ ] Update documentation with usage examples

This approach provides robust network binding while maintaining simplicity and following established CLI conventions.