# Installation

## Mac

### Homebrew (mac)

```
brew tap a2-ai/homebrew-tap
brew install a2-ai/tap/rv
```

## For Unix-like systems (Linux, macOS)

### Download the latest release

You can use the script or download the archive from the [GitHub releases page](https://github.com/a2-ai/rv/releases/latest).

```shell
curl -sSL https://raw.githubusercontent.com/A2-ai/rv/refs/heads/main/scripts/install.sh | bash

rv --version
```

## For Windows

### Download the latest release

For now, you can download the latest `x86_64-pc-windows-msvc` zip archive from the [GitHub releases page](https://github.com/a2-ai/rv/releases/latest) and extract it to a directory of your choice.

### Add the `rv` binary to your PATH

```powershell
$env:Path += ";C:\path\to\rv"

.\rv.exe --version
```
