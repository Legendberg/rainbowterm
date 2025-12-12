#!/usr/bin/env bash
# Test script for RainbowTerm profiles

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "================================================================================"
echo "RainbowTerm Profile Testing"
echo "================================================================================"
echo ""

# Check if rt is installed
if ! command -v rt &> /dev/null; then
    echo "Error: 'rt' command not found. Install with:"
    echo "  cargo install --path ."
    exit 1
fi

# Function to test a profile
test_profile() {
    local profile=$1
    local file=$2
    
    echo "--------------------------------------------------------------------------------"
    echo "Testing: $profile profile"
    echo "File: $file"
    echo "--------------------------------------------------------------------------------"
    
    if [ ! -f "$file" ]; then
        echo "Error: Test file not found: $file"
        return 1
    fi
    
    if [ "$profile" = "base" ]; then
        cat "$file" | rt --profile base
    else
        cat "$file" | rt --profile "$profile"
    fi
    
    echo ""
}

# Parse arguments
case "${1:-all}" in
    juniper|j)
        test_profile "juniper" "$SCRIPT_DIR/juniper-sample.txt"
        ;;
    cisco|c)
        test_profile "cisco" "$SCRIPT_DIR/cisco-sample.txt"
        ;;
    arista|a)
        test_profile "arista" "$SCRIPT_DIR/arista-sample.txt"
        ;;
    base|b)
        echo "Testing base profile with all files..."
        test_profile "base" "$SCRIPT_DIR/juniper-sample.txt"
        test_profile "base" "$SCRIPT_DIR/cisco-sample.txt"
        test_profile "base" "$SCRIPT_DIR/arista-sample.txt"
        ;;
    all|*)
        test_profile "juniper" "$SCRIPT_DIR/juniper-sample.txt"
        test_profile "cisco" "$SCRIPT_DIR/cisco-sample.txt"
        test_profile "arista" "$SCRIPT_DIR/arista-sample.txt"
        ;;
esac

echo "================================================================================"
echo "Testing complete!"
echo ""
echo "Usage: $0 [juniper|cisco|arista|base|all]"
echo "  juniper (j) - Test Juniper JunOS profile"
echo "  cisco (c)   - Test Cisco IOS/IOS-XE/NX-OS profile"  
echo "  arista (a)  - Test Arista EOS profile"
echo "  base (b)    - Test base profile with all files"
echo "  all         - Test all profiles (default)"
echo "================================================================================"
