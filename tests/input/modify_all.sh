#!/usr/bin/env bash

# Utility for updating all tests when the format changes

OLD="# HELP zpool_scan_state Scan status: UnknownMissing = 0, Unrecognized = 1, ScrubRepaired = 10, Resilvered = 15, ScrubInProgress = 30, ScrubCancelled = 35"
NEW="# HELP zpool_scan_state Scan status: UnknownMissing = 0, Unrecognized = 1, ScrubRepaired = 10, Resilvered = 15, ScrubInProgress = 30, ScrubCanceled = 35"

sed -i "s/${OLD}/${NEW}/" output*.txt ../../src/bin/output-integration.txt
