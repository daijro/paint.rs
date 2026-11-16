#pragma once
// Linux build shim for pdn-avif's AvifNative wrapper: provide the C runtime types and
// functions the Windows SDK/CRT headers supply implicitly, and neutralise the Windows-
// only SAL/DLL markers, so the extern "C" entry points build and export on Linux x64.
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <cmath>
#define __declspec(x)
#define __stdcall
#define __cdecl

// Windows CRT bounds-checked copy (returns 0 on success, like errno_t).
static inline int memcpy_s(void* dest, size_t destSize, const void* src, size_t count)
{
    if (count > destSize) return 22; // ERANGE
    std::memcpy(dest, src, count);
    return 0;
}
