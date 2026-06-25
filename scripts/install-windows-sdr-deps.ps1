# Install RTL-SDR (vcpkg) and build libairspyhf for Windows CI / local MSVC builds.
# Sets PKG_CONFIG_PATH, PATH, HFSDR_DEPS_PREFIX, and VCPKG_ROOT (via GITHUB_ENV when present).

$ErrorActionPreference = "Stop"

function Add-GithubEnv([string]$Name, [string]$Value) {
    if ($env:GITHUB_ENV) {
        "${Name}=${Value}" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
    }
}

$tempRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } elseif ($env:TEMP) { $env:TEMP } else { "C:\hfsdr-build" }

$vcpkgRoot = if ($env:VCPKG_ROOT) { $env:VCPKG_ROOT } else { Join-Path $tempRoot "vcpkg" }
$depsPrefix = if ($env:HFSDR_DEPS_PREFIX) { $env:HFSDR_DEPS_PREFIX } else { Join-Path $tempRoot "hfsdr-deps" }
$triplet = "x64-windows"

if (-not (Test-Path $vcpkgRoot)) {
    git clone --depth 1 https://github.com/microsoft/vcpkg.git $vcpkgRoot
    & (Join-Path $vcpkgRoot "bootstrap-vcpkg.bat") -disableMetrics
}

$vcpkg = Join-Path $vcpkgRoot "vcpkg.exe"
& $vcpkg install `
    "pkgconf:$triplet" `
    "libusb:$triplet" `
    "pthreads:$triplet" `
    "rtlsdr:$triplet"

$airspySrc = Join-Path $tempRoot "airspyhf"
if (-not (Test-Path $airspySrc)) {
    git clone --depth 1 https://github.com/airspy/airspyhf.git $airspySrc
}

$buildDir = Join-Path $airspySrc "build"
cmake -S $airspySrc -B $buildDir `
    -A x64 `
    -DCMAKE_TOOLCHAIN_FILE=(Join-Path $vcpkgRoot "scripts/buildsystems/vcpkg.cmake") `
    -DCMAKE_INSTALL_PREFIX=$depsPrefix
cmake --build $buildDir --config Release --target airspyhf
cmake --install $buildDir --config Release

$pkgConfigPaths = @(
    (Join-Path $depsPrefix "lib/pkgconfig"),
    (Join-Path $vcpkgRoot "installed/$triplet/lib/pkgconfig")
) -join ";"

$pathAdditions = @(
    (Join-Path $depsPrefix "bin"),
    (Join-Path $vcpkgRoot "installed/$triplet/bin")
) -join ";"

$env:VCPKG_ROOT = $vcpkgRoot
$env:HFSDR_DEPS_PREFIX = $depsPrefix
$env:PKG_CONFIG_PATH = $pkgConfigPaths
$env:PATH = "$pathAdditions;$env:PATH"

Add-GithubEnv "VCPKG_ROOT" $vcpkgRoot
Add-GithubEnv "HFSDR_DEPS_PREFIX" $depsPrefix
Add-GithubEnv "PKG_CONFIG_PATH" $pkgConfigPaths
Add-GithubEnv "PATH" $env:PATH

Write-Host "Windows SDR deps ready:"
Write-Host "  VCPKG_ROOT=$vcpkgRoot"
Write-Host "  HFSDR_DEPS_PREFIX=$depsPrefix"
