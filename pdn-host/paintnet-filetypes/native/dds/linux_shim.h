#pragma once
// Linux build shim for the DDS FileType native helper (DdsFileTypePlusIO).
//
// The heavy lifting — HRESULT, COM, GUID, DXGI_FORMAT, the SAL annotations and the
// DirectXMath SIMD types — comes from the pinned Microsoft header packages that
// build-deps.sh fetches: DirectX-Headers (wsl/winadapter.h -> wsl/stubs/*) and
// DirectXMath. This file only neutralises the MSVC markers those packages leave in
// place and supplies the handful of <Windows.h> helpers the wrapper uses.
//
// It is force-included (cc -include) ahead of every translation unit, so its macros
// are in effect before any vendored header is parsed.

#ifndef _WIN32

// MSVC storage-class marker. The wrapper tags its exports __declspec(dllexport);
// on Linux the .so is linked with default visibility, so this is a no-op. (__stdcall
// and __cdecl are already emptied by the DirectX-Headers stubs/basetsd.h.)
#ifndef __declspec
#define __declspec(x)
#endif

// DDS fourCC packing (normally from Windows' mmsystem.h). Guarded: DirectXTex's DDS.h
// defines its own if absent, and we must not clash.
#ifndef MAKEFOURCC
#define MAKEFOURCC(ch0, ch1, ch2, ch3) \
    ((uint32_t)(uint8_t)(ch0)        | ((uint32_t)(uint8_t)(ch1) << 8) | \
    ((uint32_t)(uint8_t)(ch2) << 16) | ((uint32_t)(uint8_t)(ch3) << 24))
#endif

// Win32 error codes the wrapper and the DDS callback-I/O path map through
// HRESULT_FROM_WIN32, plus the macro itself (FACILITY_WIN32 = 7) and the SetFilePointer
// origin. basetsd.h already defines the HRESULT S_* / E_* values; these are the standard
// Win32 numeric codes, and must match the values the managed IOCallbacks return on EOF.
#ifndef ERROR_NOT_FOUND
#define ERROR_NOT_FOUND          1168L
#endif
#ifndef ERROR_NOT_SUPPORTED
#define ERROR_NOT_SUPPORTED      50L
#endif
#ifndef ERROR_BAD_FORMAT
#define ERROR_BAD_FORMAT         11L
#endif
#ifndef ERROR_INVALID_DATA
#define ERROR_INVALID_DATA       13L
#endif
#ifndef ERROR_HANDLE_EOF
#define ERROR_HANDLE_EOF         38L
#endif
#ifndef ERROR_FILE_TOO_LARGE
#define ERROR_FILE_TOO_LARGE     223L
#endif
#ifndef ERROR_ARITHMETIC_OVERFLOW
#define ERROR_ARITHMETIC_OVERFLOW 534L
#endif
#ifndef FILE_BEGIN
#define FILE_BEGIN   0
#define FILE_CURRENT 1
#define FILE_END     2
#endif
#ifndef HRESULT_FROM_WIN32
#define HRESULT_FROM_WIN32(x) \
    ((HRESULT)(x) <= 0 ? (HRESULT)(x) \
                       : (HRESULT)((((unsigned long)(x)) & 0x0000FFFF) | (7UL << 16) | 0x80000000UL))
#endif

#endif // !_WIN32
