#!/usr/bin/env bash

# Utility for updating all tests when the format changes

OLD="# Pool status description: Normal = 0, Unrecognized = 1, SufficientReplicasForMissing = 10, DataCorruption = 50"
NEW="# Pool status description: Normal = 0, Unrecognized = 1, FeaturesAvailable = 5, SufficientReplicasForMissing = 10, DataCorruption = 50"

sed -i "s/${OLD}/${NEW}/" output*.txt ../../src/bin/output-integration.txt
