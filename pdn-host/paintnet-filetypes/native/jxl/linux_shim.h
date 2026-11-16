#pragma once
// Linux build shim for pdn-jpegxl's JxlFileTypeIO wrapper: provide the CRT types and
// functions the Windows SDK/CRT headers supply implicitly, mark the exports, and
// neutralise the Windows SAL/DLL markers, so the extern "C" entry points build and
// export on Linux x64.
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <cmath>
#include <cstdio>
#define JXLFILETYPEIO_EXPORTS 1
#define __declspec(x)
#define __stdcall
#define __cdecl

static inline int memcpy_s(void* dest, size_t destSize, const void* src, size_t count)
{ if (count > destSize) return 22; std::memcpy(dest, src, count); return 0; }
// 3-arg template form strncpy_s(dest[N], src, count): copy count chars, NUL-terminate.
static inline int strncpy_s(char* dest, const char* src, size_t count)
{ std::strncpy(dest, src, count); dest[count] = '\0'; return 0; }

#include <cstdarg>
static inline int _vscprintf(const char* format, va_list args) { return vsnprintf(nullptr, 0, format, args); }
static inline int vsprintf_s(char* dest, const char* format, va_list args) { return vsprintf(dest, format, args); }
// Windows provides global min/max; supply them as templates (not macros, which would
// clobber std::...::max member declarations in the C++ headers). Return by value: a
// `decltype(a<b?a:b)` return type is a reference to the local parameter (dangling).
template <class A, class B> static inline auto min(A a, B b) { return a < b ? a : b; }
template <class A, class B> static inline auto max(A a, B b) { return a > b ? a : b; }
