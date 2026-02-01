#!/bin/bash

# Script to unmount rustftpfs filesystems

MOUNTPOINT="${1:-/mnt/ftp}"

echo "Unmounting: $MOUNTPOINT"

# Try fusermount first (for regular users)
if command -v fusermount &> /dev/null; then
    fusermount -u "$MOUNTPOINT" 2>/dev/null
    if [ $? -eq 0 ]; then
        echo "Successfully unmounted using fusermount"
        exit 0
    fi
fi

# Try umount (for root or if fusermount fails)
if command -v umount &> /dev/null; then
    umount "$MOUNTPOINT" 2>/dev/null
    if [ $? -eq 0 ]; then
        echo "Successfully unmounted using umount"
        exit 0
    fi
fi

# Check if still mounted
if mountpoint -q "$MOUNTPOINT" 2>/dev/null; then
    echo "Error: Failed to unmount $MOUNTPOINT"
    echo "The mountpoint may still be in use. Check with:"
    echo "  lsof '$MOUNTPOINT'"
    echo "  fuser -m '$MOUNTPOINT'"
    exit 1
else
    echo "Mountpoint is not mounted"
fi