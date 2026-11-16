// The DDS saver references the GPU (ID3D11Device) overload of CompressEx in its
// BC6H/BC7 branch, but that branch is dead on Linux: DirectComputeHelper reports no
// device, so the call is never made. We still need the symbol to satisfy the linker
// (DirectXTexCompressGPU.cpp is not built — it needs the D3D11 runtime). This stub
// provides exactly the overload the wrapper names.
#include <d3d11.h>
#include "DirectXTex.h"

namespace DirectX
{
    HRESULT __cdecl CompressEx(
        ID3D11Device* /*pDevice*/, const Image* /*srcImages*/, size_t /*nimages*/,
        const TexMetadata& /*metadata*/, DXGI_FORMAT /*format*/, const CompressOptions& /*options*/,
        ScratchImage& /*cImages*/, std::function<bool __cdecl(size_t, size_t)> /*statusCallBack*/)
    {
        return E_NOTIMPL; // unreachable: no compute device on Linux
    }
}
