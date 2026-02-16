#!/bin/bash
# Installation script for llamafile-builder snap
# Generated: Feb 16 2026 10:30 (Classic Confinement - No Sandbox)

set -e

echo "============================================================"
echo "Installing llamafile-builder snap"
echo "============================================================"
echo ""

# Find the most recent snap file
SNAP_FILE=$(ls -t llamafile-builder_*.snap 2>/dev/null | head -n1)

if [ -z "$SNAP_FILE" ]; then
  echo "Error: No snap file found in current directory"
  echo "Please run 'snapcraft' to build the snap first"
  exit 1
fi

echo "Found snap file: $SNAP_FILE"
echo ""

# Step 1: Stop any running instances
echo "Step 1: Stopping any running instances..."
killall llamafile-builder 2>/dev/null || true
echo "✓ Done"
echo ""

# Step 2: Remove old version
echo "Step 2: Removing old version..."
sudo snap remove llamafile-builder 2>/dev/null || echo "  (No previous version found)"
echo "✓ Done"
echo ""

# Step 3: Install new version (classic confinement)
echo "Step 3: Installing new version with classic confinement..."
sudo snap install "$SNAP_FILE" --dangerous --classic
echo "✓ Done"
echo ""

# Step 4: Verify installation
echo "Step 4: Verifying installation..."
snap list llamafile-builder
echo ""

echo "============================================================"
echo "✓ Installation complete!"
echo "============================================================"
echo ""
echo "To launch the application, run:"
echo "  llamafile-builder"
echo ""
echo "To test with diagnostic output, run:"
echo "  llamafile-builder 2>&1 | tee /tmp/app-launch.log"
echo ""
