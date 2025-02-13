#!/bin/bash

# Define directories
LIBRARY_DIR="./rv"
LOCKFILE_DIR="./rv.lock"
export XDG_CACHE_HOME=$(pwd)/cache
CACHE_DIR="./cache"
RV_DIR="$HOME/projects/rv"
RV_CMD="$RV_DIR/target/release/rv sync -vvv"
TEST_DIR="./simple_source"

# Create the test directory if it doesn't exist
mkdir -p "$TEST_DIR"

# Set RV_LINK_MODE to symlink
delete_dirs() {
    rm -rf "$LIBRARY_DIR" "$LOCKFILE_DIR" "$CACHE_DIR"
}

# Function to run rv command and log output with time
run_and_log() {
    local log_file="$1"
    echo "$log_file"
    time -p { $RV_CMD; } &> "$log_file"
}

cargo build --release --features=cli
echo "Git SHA: $(git -C $RV_DIR rev-parse HEAD)"

# Initial cleanup
delete_dirs

# Run rv command and log output
export RV_LINK_MODE=symlink
run_and_log "$TEST_DIR/no_cache_and_no_lockfile.log"

# Remove library and lockfile, then rerun rv command and log output
rm -rf "$LIBRARY_DIR" "$LOCKFILE_DIR"
run_and_log "$TEST_DIR/cache_no_lockfile.log"

# Remove cache and library, then rerun rv command and log output
rm -rf "$CACHE_DIR" "$LIBRARY_DIR"
run_and_log "$TEST_DIR/lockfile_no_cache.log"

# Remove the cache and make sure the library is reinstalled from failed symlinks
rm -rf "$CACHE_DIR"
run_and_log "$TEST_DIR/no_cache_library.log"

# Remove library, then rerun rv command and log output
rm -rf "$LIBRARY_DIR"
run_and_log "$TEST_DIR/lockfile_and_cache.log"

delete_dirs