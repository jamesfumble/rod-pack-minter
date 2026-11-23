# One-off Windows set-up for building Solana programs natively (no WSL).
# Installs nothing by itself; it checks the tools and applies the platform-tools workaround.
# Needs: Rust (rustup, MSVC), Visual Studio Build Tools, Solana CLI 4.1.2 (Agave Windows build), Anchor CLI 1.2.2.
$ErrorActionPreference = 'Stop'
$solanaBin = "$env:USERPROFILE\.local\share\solana\install\active_release\bin"
if (-not ($env:Path -like "*$solanaBin*")) { $env:Path = "$solanaBin;$env:Path" }
rustc --version; solana --version; anchor --version

# cargo-build-sbf 4.1.0 looks for rust\bin\rustc (no .exe) when the platform-tools version is not its default (v1.54)
# and panics otherwise (toolchain.rs:357). Anchor 1.2.2 uses v1.57. Copying rustc.exe to rustc fixes it.
$tools = 'v1.57'
$bin = "$env:USERPROFILE\.cache\solana\$tools\platform-tools\rust\bin"
if (-not (Test-Path "$bin\rustc.exe")) {
    Write-Host "Platform tools $tools not downloaded yet: run 'anchor build' once (it downloads ~580 MB and fails), then run this script again."
    exit 0
}
if (-not (Test-Path "$bin\rustc")) { Copy-Item "$bin\rustc.exe" "$bin\rustc"; Write-Host "Applied the rustc workaround for $tools" }
else { Write-Host "Workaround already in place for $tools" }
