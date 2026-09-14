# Fetches the bundled llama.cpp runtime into src-tauri/binaries/llama.
#
# Pinned on purpose: llama.cpp publishes several builds a day, so an unpinned
# fetch ships a binary nobody tested. Vulkan rather than CUDA because it covers
# NVIDIA, AMD and Intel in 32MB and falls back to CPU when it finds no device;
# CUDA is 150-254MB plus a 391MB runtime and would have to be a download.

$ErrorActionPreference = 'Stop'

$build = 'b10897'
$asset = "llama-$build-bin-win-vulkan-x64.zip"
$url = "https://github.com/ggml-org/llama.cpp/releases/download/$build/$asset"

$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root 'src-tauri\binaries\llama'
$zip = Join-Path $env:TEMP $asset

if (Test-Path (Join-Path $dest 'llama-server.exe')) {
    Write-Host "Already present at $dest. Delete it to re-fetch."
    exit 0
}

Write-Host "Fetching $asset ..."
# GitHub's release downloads intermittently answer 504.
for ($attempt = 1; ; $attempt++) {
    try {
        Invoke-WebRequest -Uri $url -OutFile $zip
        break
    } catch {
        if ($attempt -ge 5) { throw }
        Write-Host "Attempt $attempt failed: $($_.Exception.Message). Retrying in 10s ..."
        Start-Sleep -Seconds 10
    }
}

$staging = Join-Path $env:TEMP "llama-$build-staging"
if (Test-Path $staging) { Remove-Item $staging -Recurse -Force }
Expand-Archive -Path $zip -DestinationPath $staging -Force

New-Item -ItemType Directory -Force -Path $dest | Out-Null

# Every DLL, because llama.cpp loads the ggml backend and the CPU variant that
# matches the host at runtime. Only llama-server among the executables.
Get-ChildItem $staging -Recurse -File | Where-Object {
    $_.Extension -ne '.exe' -or $_.Name -eq 'llama-server.exe'
} | ForEach-Object {
    Copy-Item $_.FullName -Destination (Join-Path $dest $_.Name) -Force
}

Remove-Item $staging -Recurse -Force
Remove-Item $zip -Force

$size = (Get-ChildItem $dest -File | Measure-Object Length -Sum).Sum / 1MB
Write-Host ("Done: {0:N1} MB in {1}" -f $size, $dest)
& (Join-Path $dest 'llama-server.exe') --list-devices
