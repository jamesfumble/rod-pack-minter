# Downloads the external program binaries the LiteSVM tests load (read-only `solana program dump`).
# Usage: .\scripts\fetch-fixtures.ps1 [-Cluster devnet|mainnet-beta]
param([string]$Cluster = 'devnet')
$ErrorActionPreference = 'Stop'
$dir = Join-Path $PSScriptRoot '..\tests\fixtures'
New-Item -ItemType Directory -Force $dir | Out-Null
$programs = @{
    'mpl_bubblegum'           = 'BGUMAp9Gq7iTEuizy4pqaxsTyUCBK68MDfK752saRPUY'
    'spl_account_compression' = 'cmtDvXumGCrqC1Age74AVPhSRVXJMd8PJS91L8KbNCK'
    'spl_noop'                = 'noopb9bkMVfRPU8AsbpTUg8AQkHtKwMYZiFUjNRtMmV'
    'mpl_token_metadata'      = 'metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s'
}
foreach ($name in $programs.Keys) {
    solana program dump -u $Cluster $programs[$name] (Join-Path $dir "$name.so") | Out-Null
    Write-Host "fetched $name"
}
