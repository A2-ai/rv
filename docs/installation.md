# Installation

## Mac

### Homebrew (mac)

```
brew tap a2-ai/homebrew-tap
brew install rv
```

## For Unix-like systems (Linux, macOS)

### Download the latest release

```shell
curl -sSL https://raw.githubusercontent.com/a2-ai/rv/blob/main/scripts/install.sh | bash
```

### Verify installation
```shell
rv --version
```


## For Windows

### Download the latest release

For now, you can download the latest `x86_64-pc-windows-msvc` zip archive from the [GitHub releases page](https://github.com/a2-ai/rv/releases/latest) and extract it to a directory of your choice.

### Add the `rv` binary to your PATH

```powershell
$env:Path += ";C:\path\to\rv"
```

### Verify installation
```powershell
.\rv.exe --version
```
