// Neutralises the Windows-only spellings in the vendored pdn-webp wrapper so the
// exact same source compiles on Linux/macOS. Force-included; upstream is unedited.
#pragma once
#include <cstddef>
#include <limits>
#include <new>
using std::nullptr_t;      // MSVC exposes this in the global namespace; libstdc++ does not.
#define __stdcall          // x86-64 SysV has one calling convention; the tag is a no-op.
#define __declspec(x)      // exports come from default ELF visibility instead.
#define WEBP_EXPORTS 1
