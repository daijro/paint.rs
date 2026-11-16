#pragma once
// Stub for the plugin's GPU helper. There is no DirectCompute device on Linux, so
// ComputeDeviceAvailable() is always false and the DDS saver takes its CPU path
// (DirectXTex::CompressEx without a device). The adapter pointer is always null.
#include <d3d11.h>

class DirectComputeHelper
{
public:
    explicit DirectComputeHelper(IDXGIAdapter* /*directComputeAdapter*/) noexcept {}
    DirectComputeHelper(const DirectComputeHelper&) = delete;
    DirectComputeHelper& operator=(const DirectComputeHelper&) = delete;

    bool ComputeDeviceAvailable() const noexcept { return false; }
    ID3D11Device* GetComputeDevice() const noexcept { return nullptr; }
    void Release() noexcept {}
};
