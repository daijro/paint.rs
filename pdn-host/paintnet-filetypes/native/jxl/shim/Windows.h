#pragma once
// Minimal Windows.h stand-in: the JXL output processor returns COM HRESULTs.
typedef int HRESULT;
#define S_OK ((HRESULT)0)
#define E_ABORT ((HRESULT)0x80004004)
#define E_OUTOFMEMORY ((HRESULT)0x8007000E)
#define E_FAIL ((HRESULT)0x80004005)
#define E_INVALIDARG ((HRESULT)0x80070057)
#define SUCCEEDED(hr) ((HRESULT)(hr) >= 0)
#define FAILED(hr) ((HRESULT)(hr) < 0)
