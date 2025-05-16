#!/bin/bash
set -e

# Ensure target directory exists and cd into it
mkdir -p ~/.local/bin && cd ~/.local/bin

# Determine OS and Architecture
os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
if [ "$arch" = "arm64" ]; then arch="aarch64"; elif [ "$arch" = "x86_64" ]; then arch="x86_64"; fi

# Adjust OS string for macOS asset naming convention
if [ "$os" = "darwin" ]; then
    os_pattern="apple-darwin"
else
    os_pattern="unknown-linux-gnu"
fi

# Fetch the latest release data from GitHub API and extract the download URL for the matching asset
echo "Fetching download URL for $arch-$os_pattern..."
asset_url=$(curl -s https://api.github.com/repos/a2-ai/rv/releases/latest | grep -o "https://github.com/A2-ai/rv/releases/download/.*$arch-$os_pattern.tar.gz")

# Check if URL was found
if [ -z "$asset_url" ]; then
    echo "Error: Could not find a suitable release asset for your system ($arch-$os_pattern) on GitHub." >&2
    echo "Please check available assets at https://github.com/a2-ai/rv/releases/latest" >&2
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
