#!/bin/sh
# AppRun hook: isolate bundled SoapySDR modules and libraries from the host system.

if [ -z "${APPDIR:-}" ]; then
    return 0 2>/dev/null || exit 0
fi

SOAPY_MODULES="${APPDIR}/usr/lib/SoapySDR/modules0.8"
if [ -d "${SOAPY_MODULES}" ]; then
    export SOAPY_SDR_PLUGIN_PATH="${SOAPY_MODULES}"
fi

prepend_ld() {
    _dir="$1"
    if [ -d "${_dir}" ]; then
        case ":${LD_LIBRARY_PATH:-}:" in
            *:"${_dir}":*) ;;
            *) export LD_LIBRARY_PATH="${_dir}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}" ;;
        esac
    fi
}

prepend_ld "${APPDIR}/usr/lib"
prepend_ld "${APPDIR}/usr/lib/x86_64-linux-gnu"
