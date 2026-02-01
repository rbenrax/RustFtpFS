#!/bin/bash

# Example script to mount an FTP filesystem using rustftpfs

# Configuration
FTP_URL="ftp://ftp.gnu.org"
MOUNTPOINT="/mnt/gnu"
USERNAME=""
PASSWORD=""
PORT=""
OPTIONS=""

# Check if running as root
if [[ $EUID -eq 0 ]]; then
   echo "This script should not be run as root for security reasons"
   exit 1
fi

# Check if FUSE is available
if ! command -v fusermount &> /dev/null && ! command -v umount &> /dev/null; then
    echo "FUSE utilities not found. Please install FUSE first."
    exit 1
fi

# Create mountpoint if it doesn't exist
mkdir -p "$MOUNTPOINT"

# Build command
CMD="cargo run --release --"
CMD="$CMD \"$FTP_URL\" \"$MOUNTPOINT\""

# Add credentials if provided
if [[ -n "$USERNAME" ]]; then
    CMD="$CMD --user \"$USERNAME\""
fi

if [[ -n "$PASSWORD" ]]; then
    CMD="$CMD --password \"$PASSWORD\""
fi

if [[ -n "$PORT" ]]; then
    CMD="$CMD --port $PORT"
fi

# Add common options
CMD="$CMD --foreground"

# Add any additional options
if [[ -n "$OPTIONS" ]]; then
    CMD="$CMD $OPTIONS"
fi

echo "Mounting FTP filesystem..."
echo "Command: $CMD"

# Execute the command
eval $CMD

echo "FTP filesystem mounted at: $MOUNTPOINT"
echo "To unmount, run: fusermount -u '$MOUNTPOINT'"
# or: umount '$MOUNTPOINT'