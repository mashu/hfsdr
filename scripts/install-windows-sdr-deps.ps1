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
$vcpkgInstalled = Join-Path $vcpkgRoot "installed/$triplet"

if (-not (Test-Path $vcpkgRoot)) {
    git clone --depth 1 https://github.com/microsoft/vcpkg.git $vcpkgRoot
    & (Join-Path $vcpkgRoot "bootstrap-vcpkg.bat") -disableMetrics
}

$vcpkg = Join-Path $vcpkgRoot "vcpkg.exe"
& $vcpkg install `
    "pkgconf:$triplet" `
    "libusb:$triplet" `
    "pthreads:$triplet" `
    "rtlsdr:$triplet" `
    "soapysdr:$triplet"

# vcpkg pkg-config + DLLs must be visible before configuring libairspyhf.
$pkgconfDir = Join-Path $vcpkgInstalled "tools/pkgconf"
$vcpkgBin = Join-Path $vcpkgInstalled "bin"
$vcpkgPkgConfig = Join-Path $vcpkgInstalled "lib/pkgconfig"
$env:PKG_CONFIG_PATH = $vcpkgPkgConfig
$env:PATH = "$pkgconfDir;$vcpkgBin;$env:PATH"

$airspySrc = Join-Path $tempRoot "airspyhf"
if (-not (Test-Path $airspySrc)) {
    git clone --depth 1 https://github.com/airspy/airspyhf.git $airspySrc
}

$buildDir = Join-Path $airspySrc "build"
$depsLib = Join-Path $depsPrefix "lib"
$depsBin = Join-Path $depsPrefix "bin"
$depsPkgConfig = Join-Path $depsLib "pkgconfig"
New-Item -ItemType Directory -Force -Path $depsLib, $depsBin, $depsPkgConfig | Out-Null

# airspyhf ships a custom FindThreads.cmake that searches for pthreadVC2; vcpkg
# pthreads 3.x installs pthreadVC3. Point CMake at the vcpkg install explicitly.
$pthreadsInclude = Join-Path $vcpkgInstalled "include"
$pthreadsLibDir = Join-Path $vcpkgInstalled "lib"
$pthreadsLib = Get-ChildItem -Path $pthreadsLibDir -Filter "pthreadVC*.lib" |
    Where-Object { $_.Name -notmatch 'd\.lib$' } |
    Sort-Object Name |
    Select-Object -First 1
if (-not $pthreadsLib) {
    throw "pthreadVC*.lib not found under $pthreadsLibDir"
}
if (-not (Test-Path (Join-Path $pthreadsInclude "pthread.h"))) {
    throw "pthread.h not found under $pthreadsInclude"
}

$toolchainFile = Join-Path $vcpkgRoot "scripts/buildsystems/vcpkg.cmake"
cmake -S $airspySrc -B $buildDir `
    -A x64 `
    "-DCMAKE_TOOLCHAIN_FILE=$toolchainFile" `
    -DCMAKE_INSTALL_PREFIX=$depsPrefix `
    -DTHREADS_USE_PTHREADS_WIN32=ON `
    "-DTHREADS_PTHREADS_INCLUDE_DIR=$pthreadsInclude" `
    "-DTHREADS_PTHREADS_WIN32_LIBRARY=$($pthreadsLib.FullName)"
cmake --build $buildDir --config Release --target airspyhf
cmake --install $buildDir --config Release

# WIN32 CMake rules install the DLL to bin/ only; the MSVC import library stays in
# the build tree. Copy it into lib/ so rustc and pkg-config can link.
$importLib = Get-ChildItem -Path $buildDir -Recurse -Filter "airspyhf.lib" |
    Where-Object { $_.FullName -match "\\Release\\" } |
    Select-Object -First 1
if (-not $importLib) {
    throw "airspyhf.lib not found under $buildDir after build"
}
Copy-Item $importLib.FullName (Join-Path $depsLib "airspyhf.lib") -Force

# CMake install may leave the DLL in the build tree only; ensure bin/ has it for packaging.
$airspyDllDest = Join-Path $depsBin "airspyhf.dll"
if (-not (Test-Path $airspyDllDest)) {
    $builtDll = Get-ChildItem -Path $buildDir -Recurse -Filter "airspyhf.dll" |
        Where-Object { $_.FullName -match "\\Release\\" } |
        Select-Object -First 1
    if (-not $builtDll) {
        throw "airspyhf.dll not found under $buildDir after build"
    }
    Copy-Item $builtDll.FullName $airspyDllDest -Force
}

$pcFile = Join-Path $depsPkgConfig "libairspyhf.pc"
if (Test-Path $pcFile) {
    # Generated .pc references -lm (Unix only).
    (Get-Content $pcFile -Raw) -replace '\s+-lm', '' | Set-Content $pcFile -NoNewline
}

$pkgConfigPaths = "$depsPkgConfig;$vcpkgPkgConfig"
$pathAdditions = "$depsBin;$vcpkgBin"

$env:VCPKG_ROOT = $vcpkgRoot
$env:HFSDR_DEPS_PREFIX = $depsPrefix
$env:PKG_CONFIG_PATH = $pkgConfigPaths
$env:PATH = "$pathAdditions;$pkgconfDir;$env:PATH"

Add-GithubEnv "VCPKG_ROOT" $vcpkgRoot
Add-GithubEnv "HFSDR_DEPS_PREFIX" $depsPrefix
Add-GithubEnv "PKG_CONFIG_PATH" $pkgConfigPaths
Add-GithubEnv "PATH" $env:PATH

$required = @(
    (Join-Path $depsLib "airspyhf.lib"),
    (Join-Path $depsBin "airspyhf.dll"),
    (Join-Path $vcpkgInstalled "lib/rtlsdr.lib"),
    (Join-Path $vcpkgInstalled "bin/rtlsdr.dll"),
    (Join-Path $vcpkgInstalled "lib/SoapySDR.lib"),
    (Join-Path $vcpkgInstalled "bin/SoapySDR.dll")
)
foreach ($path in $required) {
    if (-not (Test-Path $path)) {
        throw "Missing required library: $path"
    }
}

Write-Host "Windows SDR deps ready:"
Write-Host "  VCPKG_ROOT=$vcpkgRoot"
Write-Host "  HFSDR_DEPS_PREFIX=$depsPrefix"
Write-Host "  airspyhf.lib -> $(Join-Path $depsLib 'airspyhf.lib')"
Write-Host "  rtlsdr.lib   -> $(Join-Path $vcpkgInstalled 'lib/rtlsdr.lib')"
Write-Host "  SoapySDR.dll -> $(Join-Path $vcpkgInstalled 'bin/SoapySDR.dll')"
