#!/bin/bash
set -e

# Ensure target directory exists and cd into it
mkdir -p ~/.local/bin && cd ~/.local/bin

# Determine OS and Architecture
os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
if [ "$arch" = "arm64" ]; then arch="aarch64"; elif [ "$arch" = "x86_64" ]; then arch="x86_64"; fi

# Function to compare version numbers
version_compare() {
    # Returns 0 if $1 >= $2, 1 otherwise
    printf '%s\n%s\n' "$2" "$1" | sort -V -C
}

# Function to get glibc version
get_glibc_version() {
    # Try multiple methods to get glibc version
    if command -v ldd >/dev/null 2>&1; then
        # Method 1: Use ldd --version
        glibc_version=$(ldd --version 2>/dev/null | head -n1 | grep -oE '[0-9]+\.[0-9]+' | head -n1)
        if [ -n "$glibc_version" ]; then
            echo "$glibc_version"
            return 0
        fi
    fi

    # Method 2: Check if glibc library exists and try to get version
    if [ -f /lib/x86_64-linux-gnu/libc.so.6 ] || [ -f /lib64/libc.so.6 ] || [ -f /lib/libc.so.6 ]; then
        for lib_path in /lib/x86_64-linux-gnu/libc.so.6 /lib64/libc.so.6 /lib/libc.so.6 /lib/aarch64-linux-gnu/libc.so.6; do
            if [ -f "$lib_path" ]; then
                glibc_version=$("$lib_path" 2>/dev/null | head -n1 | grep -oE '[0-9]+\.[0-9]+' | head -n1)
                if [ -n "$glibc_version" ]; then
                    echo "$glibc_version"
                    return 0
                fi
            fi
        done
    fi

    # Method 3: Use getconf if available
    if command -v getconf >/dev/null 2>&1; then
        glibc_version=$(getconf GNU_LIBC_VERSION 2>/dev/null | grep -oE '[0-9]+\.[0-9]+' | head -n1)
        if [ -n "$glibc_version" ]; then
            echo "$glibc_version"
            return 0
        fi
    fi

    # If all methods fail, return empty
    echo ""
}

# Determine the appropriate target based on OS
if [ "$os" = "darwin" ]; then
    os_pattern="apple-darwin"
    echo "Detected macOS, using apple-darwin target"
elif [ "$os" = "linux" ]; then
    # Check glibc version to determine if we should use musl
    glibc_version=$(get_glibc_version)

    if [ -n "$glibc_version" ]; then
        echo "Detected glibc version: $glibc_version"
        if version_compare "$glibc_version" "2.31"; then
            os_pattern="unknown-linux-gnu"
            echo "glibc >= 2.31, using gnu target"
        else
            os_pattern="unknown-linux-musl"
            echo "glibc < 2.31, using musl target for better compatibility"
        fi
    else
        echo "Could not determine glibc version, defaulting to musl target for better compatibility"
        os_pattern="unknown-linux-musl"
    fi
else
    # Default fallback for other Unix-like systems
    echo "Unknown OS: $os, defaulting to linux-gnu target"
    os_pattern="unknown-linux-gnu"
fi

# Fetch the latest release data from GitHub API and extract the download URL for the matching asset
echo "Fetching download URL for $arch-$os_pattern..."
asset_url=$(curl -s https://api.github.com/repos/a2-ai/rv/releases/latest | grep -o "https://github.com/A2-ai/rv/releases/download/.*$arch-$os_pattern.tar.gz")

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
    exit 1
fi

# Download the asset using curl, extract it, clean up, and make executable
echo "Downloading rv from $asset_url"
curl -L -o rv_latest.tar.gz "$asset_url" &&
    tar -xzf rv_latest.tar.gz &&
    rm rv_latest.tar.gz &&
    chmod +x rv &&
    echo "rv installed successfully to ~/.local/bin" ||
    (echo "Installation failed." >&2 && exit 1)

# Add ~/.local/bin to PATH if not already present
if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
    echo "Adding ~/.local/bin to your PATH..."
    if [[ "$SHELL" == *"bash"* ]]; then
        printf '\n%s\n' 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
        echo "Please source ~/.bashrc or open a new terminal."
    elif [[ "$SHELL" == *"zsh"* ]]; then
        printf '\n%s\n' 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
        echo "Please source ~/.zshrc or open a new terminal."
    elif [[ "$SHELL" == *"fish"* ]]; then
        printf '\n%s\n' 'fish_add_path "$HOME/.local/bin"' >> ~/.config/fish/config.fish
        echo "~/.local/bin added to fish path. Changes will apply to new fish shells."
    else
        echo "Could not detect shell. Please add ~/.local/bin to your PATH manually."
    fi
else
    echo "~/.local/bin is already in your PATH."
fi
