#!/bin/bash
set -e

# Parse command line arguments
VERBOSE=false
for arg in "$@"; do
    case $arg in
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        *)
            # Unknown option
            ;;
    esac
done

# Verbose logging function
log_verbose() {
    if [ "$VERBOSE" = true ]; then
        echo "[VERBOSE] $*" >&2
    fi
}

log_verbose "=== STARTING DIAGNOSTIC MODE ==="
log_verbose "Script arguments: $*"
log_verbose "VERBOSE mode enabled"

# Ensure target directory exists and cd into it
mkdir -p ~/.local/bin && cd ~/.local/bin

# Determine OS and Architecture
os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)

log_verbose "=== SYSTEM DETECTION ==="
log_verbose "Raw OS from uname -s: $(uname -s)"
log_verbose "Normalized OS: $os"
log_verbose "Raw architecture from uname -m: $(uname -m)"
log_verbose "Original arch: $arch"

if [ "$arch" = "arm64" ]; then 
    arch="aarch64"
    log_verbose "Converted arm64 to aarch64"
elif [ "$arch" = "x86_64" ]; then 
    arch="x86_64"
    log_verbose "Keeping x86_64 as is"
else
    log_verbose "Architecture $arch not explicitly handled, keeping as is"
fi

log_verbose "Final architecture: $arch"

# Function to compare version numbers
version_compare() {
    # Returns 0 if $1 >= $2, 1 otherwise
    local ver1="$1"
    local ver2="$2"
    log_verbose "Comparing versions: $ver1 >= $ver2"
    if printf '%s\n%s\n' "$ver2" "$ver1" | sort -V -C; then
        log_verbose "Version comparison result: $ver1 >= $ver2 = TRUE"
        return 0
    else
        log_verbose "Version comparison result: $ver1 >= $ver2 = FALSE"
        return 1
    fi
}

# Function to get glibc version
get_glibc_version() {
    log_verbose "=== GLIBC VERSION DETECTION ==="
    
    # Try multiple methods to get glibc version
    if command -v ldd >/dev/null 2>&1; then
        log_verbose "Method 1: ldd command is available"
        # Method 1: Use ldd --version
        ldd_output=$(ldd --version 2>/dev/null || echo "")
        log_verbose "ldd --version output: $ldd_output"
        
        glibc_version=$(echo "$ldd_output" | head -n1 | grep -oE '[0-9]+\.[0-9]+' | head -n1)
        if [ -n "$glibc_version" ]; then
            log_verbose "Method 1 SUCCESS: Found glibc version $glibc_version from ldd"
            echo "$glibc_version"
            return 0
        else
            log_verbose "Method 1 FAILED: Could not extract version from ldd output"
        fi
    else
        log_verbose "Method 1 SKIPPED: ldd command not available"
    fi

    # Method 2: Check if glibc library exists and try to get version
    log_verbose "Method 2: Checking for glibc library files"
    libc_paths=(/lib/x86_64-linux-gnu/libc.so.6 /lib64/libc.so.6 /lib/libc.so.6 /lib/aarch64-linux-gnu/libc.so.6)
    for lib_path in "${libc_paths[@]}"; do
        log_verbose "Checking path: $lib_path"
        if [ -f "$lib_path" ]; then
            log_verbose "Found library at: $lib_path"
            lib_output=$("$lib_path" 2>/dev/null || echo "")
            log_verbose "Library output: $lib_output"
            
            glibc_version=$(echo "$lib_output" | head -n1 | grep -oE '[0-9]+\.[0-9]+' | head -n1)
            if [ -n "$glibc_version" ]; then
                log_verbose "Method 2 SUCCESS: Found glibc version $glibc_version from $lib_path"
                echo "$glibc_version"
                return 0
            else
                log_verbose "Method 2 FAILED: Could not extract version from $lib_path output"
            fi
        else
            log_verbose "Library not found at: $lib_path"
        fi
    done

    # Method 3: Use getconf if available
    if command -v getconf >/dev/null 2>&1; then
        log_verbose "Method 3: getconf command is available"
        getconf_output=$(getconf GNU_LIBC_VERSION 2>/dev/null || echo "")
        log_verbose "getconf GNU_LIBC_VERSION output: $getconf_output"
        
        glibc_version=$(echo "$getconf_output" | grep -oE '[0-9]+\.[0-9]+' | head -n1)
        if [ -n "$glibc_version" ]; then
            log_verbose "Method 3 SUCCESS: Found glibc version $glibc_version from getconf"
            echo "$glibc_version"
            return 0
        else
            log_verbose "Method 3 FAILED: Could not extract version from getconf output"
        fi
    else
        log_verbose "Method 3 SKIPPED: getconf command not available"
    fi

    # Method 4: Additional diagnostics - check for musl
    log_verbose "Method 4: Checking for musl libc"
    if [ -f /lib/ld-musl-x86_64.so.1 ] || [ -f /lib/ld-musl-aarch64.so.1 ]; then
        log_verbose "MUSL detected: Found musl dynamic linker"
    else
        log_verbose "MUSL check: No musl dynamic linker found"
    fi
    
    # Check /proc/version for additional info
    if [ -f /proc/version ]; then
        log_verbose "System info from /proc/version: $(cat /proc/version)"
    fi
    
    # Check what's in /lib and /lib64
    if [ -d /lib ]; then
        log_verbose "Contents of /lib: $(ls -la /lib/ | grep -E '(libc|musl)' || echo 'No libc/musl files found')"
    fi
    if [ -d /lib64 ]; then
        log_verbose "Contents of /lib64: $(ls -la /lib64/ | grep -E '(libc|musl)' || echo 'No libc/musl files found')"
    fi

    # If all methods fail, return empty
    log_verbose "ALL METHODS FAILED: Could not determine glibc version"
    echo ""
}

# Determine the appropriate target based on OS
log_verbose "=== TARGET SELECTION ==="

if [ "$os" = "darwin" ]; then
    os_pattern="apple-darwin"
    echo "Detected macOS, using apple-darwin target"
    log_verbose "macOS detected, selected target: $os_pattern"
elif [ "$os" = "linux" ]; then
    log_verbose "Linux detected, determining gnu vs musl"
    
    # Check glibc version to determine if we should use musl
    glibc_version=$(get_glibc_version)
    
    log_verbose "glibc detection result: '$glibc_version'"

    if [ -n "$glibc_version" ]; then
        echo "Detected glibc version: $glibc_version"
        log_verbose "glibc version found: $glibc_version"
        log_verbose "Comparing against threshold 2.28"
        
        if version_compare "$glibc_version" "2.28"; then
            os_pattern="unknown-linux-gnu"
            echo "glibc >= 2.28, using gnu target"
            log_verbose "DECISION: glibc $glibc_version >= 2.28, selected gnu target"
        else
            os_pattern="unknown-linux-musl"
            echo "glibc < 2.28, using musl target for compatibility"
            log_verbose "DECISION: glibc $glibc_version < 2.28, selected musl target"
        fi
    else
        echo "Could not determine glibc version, defaulting to musl target for compatibility"
        os_pattern="unknown-linux-musl"
        log_verbose "DECISION: No glibc version detected, defaulting to musl target"
    fi
else
    # Default fallback for other Unix-like systems
    echo "Unknown OS: $os, defaulting to linux-gnu target"
    os_pattern="unknown-linux-gnu"
    log_verbose "DECISION: Unknown OS $os, defaulting to gnu target"
fi

log_verbose "Final target pattern: $arch-$os_pattern"

# Fetch the latest release data from GitHub API and extract the download URL for the matching asset
echo "Fetching download URL for $arch-$os_pattern..."
log_verbose "=== GITHUB API QUERY ==="
log_verbose "Fetching from: https://api.github.com/repos/a2-ai/rv/releases/latest"

github_response=$(curl -s https://api.github.com/repos/a2-ai/rv/releases/latest)
log_verbose "GitHub API response length: $(echo "$github_response" | wc -c) characters"

if [ "$VERBOSE" = true ]; then
    log_verbose "Available assets in release:"
    echo "$github_response" | grep -o '"browser_download_url": "[^"]*"' | sed 's/"browser_download_url": "//; s/"//' | while read -r url; do
        log_verbose "  - $(basename "$url")"
    done
fi

asset_url=$(echo "$github_response" | grep -o "https://github.com/A2-ai/rv/releases/download/.*$arch-$os_pattern.tar.gz")
log_verbose "Searching for pattern: *$arch-$os_pattern.tar.gz"
log_verbose "Found asset URL: $asset_url"

# Check if URL was found
if [ -z "$asset_url" ]; then
    echo "Error: Could not find a suitable release asset for your system ($arch-$os_pattern) on GitHub." >&2
    echo "Please check available assets at https://github.com/a2-ai/rv/releases/latest" >&2
    echo "Available targets typically include:" >&2
    echo "  - x86_64-unknown-linux-gnu" >&2
    echo "  - x86_64-unknown-linux-musl" >&2
    echo "  - aarch64-unknown-linux-gnu" >&2
    echo "  - aarch64-unknown-linux-musl" >&2
    echo "  - x86_64-apple-darwin" >&2
    echo "  - aarch64-apple-darwin" >&2
    
    if [ "$VERBOSE" = true ]; then
        log_verbose "=== DEBUGGING INFO ==="
        log_verbose "Target we searched for: $arch-$os_pattern"
        log_verbose "All download URLs found in response:"
        echo "$github_response" | grep -o '"browser_download_url": "[^"]*"' | sed 's/"browser_download_url": "//; s/"//' | while read -r url; do
            log_verbose "  $url"
        done
    fi
    
    exit 1
fi

log_verbose "=== DOWNLOAD AND INSTALLATION ==="
log_verbose "Download URL: $asset_url"

# Download the asset using curl, extract it, clean up, and make executable
echo "Downloading rv from $asset_url"
curl -L -o rv_latest.tar.gz "$asset_url" &&
    tar -xzf rv_latest.tar.gz &&
    rm rv_latest.tar.gz &&
    chmod +x rv &&
    echo "rv installed successfully to ~/.local/bin" ||
    (echo "Installation failed." >&2 && exit 1)

log_verbose "Installation completed successfully"

# Add ~/.local/bin to PATH if not already present
if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
    echo "Adding ~/.local/bin to your PATH..."
    log_verbose "~/.local/bin not in PATH, adding it"
    if [[ "$SHELL" == *"bash"* ]]; then
        printf '\n%s\n' 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
        echo "Please source ~/.bashrc or open a new terminal."
        log_verbose "Added to ~/.bashrc"
    elif [[ "$SHELL" == *"zsh"* ]]; then
        printf '\n%s\n' 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
        echo "Please source ~/.zshrc or open a new terminal."
        log_verbose "Added to ~/.zshrc"
    elif [[ "$SHELL" == *"fish"* ]]; then
        printf '\n%s\n' 'fish_add_path "$HOME/.local/bin"' >> ~/.config/fish/config.fish
        echo "~/.local/bin added to fish path. Changes will apply to new fish shells."
        log_verbose "Added to fish config"
    else
        echo "Could not detect shell. Please add ~/.local/bin to your PATH manually."
        log_verbose "Unknown shell: $SHELL"
    fi
else
    echo "~/.local/bin is already in your PATH."
    log_verbose "~/.local/bin already in PATH"
fi

log_verbose "=== SCRIPT COMPLETED ==="
