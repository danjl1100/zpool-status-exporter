#!/usr/bin/env bash

# Utility for updating all tests when the format changes

OLD="# HELP zpool_pool_status_desc Pool status description: Normal = 0, Unrecognized = 1, FeaturesAvailable = 5, SufficientReplicasForMissing = 10, DataCorruption = 50"
NEW="# HELP zpool_pool_status_desc Pool status description: Normal = 0, Unrecognized = 1, FeaturesAvailable = 5, SufficientReplicasForMissing = 10, DeviceRemoved = 15, DataCorruption = 50"

sed -i "s/${OLD}/${NEW}/" output*.txt ../../src/bin/output-integration.txt
