#pragma once
// Stand-in for <d3d11.h>. The DDS helper never touches a Direct3D device on Linux
// (no compute adapter — see DirectComputeHelper below), so the real header is not
// needed. Defining __d3d11_h__ makes DirectXTex.h expose its ID3D11Device CompressEx
// overloads, which keeps DdsFileTypePlusIO.cpp compiling unmodified; the one the
// wrapper references is satisfied by a stub (gpu_stub.cpp) and never executed.
#ifndef __d3d11_h__
#define __d3d11_h__
#endif

// Opaque stand-ins for the D3D11 types named in DirectXTex's (now visible) D3D11
// declarations. None are ever instantiated on Linux: the compute/texture entry points
// they appear in are never called, and the one the wrapper references (CompressEx) is
// a no-op stub. They only need to be declared so the signatures parse.
struct IDXGIAdapter;
struct ID3D11Device;
struct ID3D11DeviceContext;
struct ID3D11Resource;
struct ID3D11ShaderResourceView;
enum D3D11_USAGE { D3D11_USAGE_DEFAULT = 0 };
enum D3D11_RESOURCE_DIMENSION { D3D11_RESOURCE_DIMENSION_UNKNOWN = 0 };
enum D3D11_RESOURCE_MISC_FLAG { D3D11_RESOURCE_MISC_NONE = 0 };
enum D3D11_TILE_SHAPE { D3D11_TILE_SHAPE_UNKNOWN = 0 };
