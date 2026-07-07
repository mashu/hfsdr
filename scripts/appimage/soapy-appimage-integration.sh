# SoapySDR AppImage isolation — sourced from scripts/build-appimage.sh

install_soapy_apprun_hook() {
    local hook_src="${SCRIPT_DIR}/appimage/apprun-soapy-hook.sh"
    local hook_dst="${APPDIR}/apprun-hooks/soapy-isolation.sh"
    if [ ! -f "${hook_src}" ]; then
        echo "error: missing ${hook_src}" >&2
        exit 1
    fi
    mkdir -p "${APPDIR}/apprun-hooks"
    install -m 0755 "${hook_src}" "${hook_dst}"
    echo "installed AppRun hook: ${hook_dst}"
}

# Neutralize libSoapySDR's hard-coded Debian install search paths so only
# SOAPY_SDR_PLUGIN_PATH (bundled modules) is used at runtime.
patch_soapy_install_paths() {
    local -A seen=()
    local lib real base args=()
    for lib in "${LIBDIR}"/libSoapySDR.so*; do
        [[ -e "$lib" ]] || continue
        real="$(readlink -f "$lib")"
        [[ -n "${seen[$real]:-}" ]] && continue
        seen[$real]=1
        args+=("$real")
    done
    if [ "${#args[@]}" -eq 0 ]; then
        echo "warning: no libSoapySDR found in ${LIBDIR}" >&2
        return 0
    fi
    python3 "${SCRIPT_DIR}/appimage/patch-soapy-lib.py" "${args[@]}"
}

validate_bundled_soapy_modules() {
    local modules_dir="${APPDIR}/usr/lib/SoapySDR/modules0.8"
    if [ ! -d "${modules_dir}" ]; then
        echo "warning: no bundled Soapy modules at ${modules_dir}" >&2
        return 0
    fi
    local failed=0
    local ld_path="${APPDIR}/usr/lib:${APPDIR}/usr/lib/x86_64-linux-gnu"
    shopt -s nullglob
    for mod in "${modules_dir}"/*.so; do
        echo "ldd check: ${mod}"
        if LD_LIBRARY_PATH="${ld_path}:${LD_LIBRARY_PATH:-}" \
            ldd "${mod}" 2>&1 | grep -q 'not found'; then
            echo "error: missing libraries for ${mod}:" >&2
            LD_LIBRARY_PATH="${ld_path}:${LD_LIBRARY_PATH:-}" \
                ldd "${mod}" 2>&1 | grep 'not found' >&2 || true
            failed=1
        fi
    done
    shopt -u nullglob
    if [ "${failed}" -ne 0 ]; then
        echo "error: bundled Soapy module(s) have unresolved libraries" >&2
        exit 1
    fi
}

run_soapy_appimage_prep() {
    : "${APPDIR:?APPDIR must be set before run_soapy_appimage_prep}"
    patch_soapy_install_paths
    install_soapy_apprun_hook
    validate_bundled_soapy_modules
}
