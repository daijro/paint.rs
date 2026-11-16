// Byte-identity harness for the DDS native helper. Builds a 16x16 R8G8B8A8 scratch
// image with a fixed gradient, saves it to the given DXGI format through the
// callback-I/O entry points, and writes the resulting .dds. Build + run against both
// the Linux .so and the Windows helper DLL under Wine, then compare:
//   cc -O2 byteident.c -ldl -o /tmp/bi && /tmp/bi ./libDdsFileTypePlusIO_x64.so /tmp/ours.dds 28
//   x86_64-w64-mingw32-gcc -O2 byteident.c -o bi.exe
//   # with DdsFileTypePlusIO_x64.dll (+ its deps) in cwd under Wine:
//   WINEPREFIX=$HOME/.cache/pdnwine wine bi.exe DdsFileTypePlusIO_x64.dll /tmp/win.dds 28
//   cmp /tmp/ours.dds /tmp/win.dds
// DXGI_FORMAT: 28=R8G8B8A8_UNORM, 71=BC1_UNORM, 77=BC3_UNORM, 98=BC7_UNORM.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#include <windows.h>
#define LOADLIB(p) LoadLibraryA(p)
#define GETSYM(h,n) (void*)GetProcAddress((HMODULE)h,n)
#else
#include <dlfcn.h>
#define LOADLIB(p) dlopen(p, RTLD_NOW)
#define GETSYM(h,n) dlsym(h,n)
#define __stdcall
typedef uint32_t DWORD;
typedef int64_t  INT64;
typedef int32_t  INT32;
typedef int32_t  HRESULT;
#endif

struct ScratchImageData { uint8_t* pixels; size_t width, height, stride, totalImageDataSize; int32_t format; };
struct DDSSaveInfo { int32_t format, fileOptions, errorMetric, compressionSpeed; uint8_t errorDiffusionDithering; };
struct IOCallbacks {
    HRESULT(__stdcall *Read)(void* buffer, DWORD count);
    HRESULT(__stdcall *Write)(const void* buffer, DWORD count);
    HRESULT(__stdcall *Seek)(INT64 position, INT32 origin);
    HRESULT(__stdcall *GetSize)(INT64* size);
};
typedef HRESULT(__stdcall *CreateFn)(int32_t,int32_t,int32_t,int32_t,int32_t,uint8_t,void**);
typedef HRESULT(__stdcall *GetDataFn)(void*,size_t,size_t,size_t,struct ScratchImageData*);
typedef HRESULT(__stdcall *SaveFn)(const struct DDSSaveInfo*,const void*,const struct IOCallbacks*,void*,void*);
typedef void(__stdcall *DestroyFn)(void*);

// Seekable in-memory sink the Save callbacks write through.
static uint8_t g_buf[1<<22];
static int64_t g_pos, g_len;
static HRESULT __stdcall ioWrite(const void* b, DWORD n){ memcpy(g_buf+g_pos,b,n); g_pos+=n; if(g_pos>g_len)g_len=g_pos; return 0; }
static HRESULT __stdcall ioRead(void* b, DWORD n){ memcpy(b,g_buf+g_pos,n); g_pos+=n; return 0; }
static HRESULT __stdcall ioSeek(INT64 p, INT32 origin){ g_pos = origin==1?g_pos+p : origin==2?g_len+p : p; return 0; }
static HRESULT __stdcall ioGetSize(INT64* s){ *s=g_len; return 0; }

int main(int argc,char**argv){
    if(argc<4){fprintf(stderr,"usage: %s <lib> <out.dds> <dxgiFormat>\n",argv[0]);return 2;}
    void* h=LOADLIB(argv[1]); if(!h){fprintf(stderr,"load fail %s\n",argv[1]);return 2;}
    CreateFn create=(CreateFn)GETSYM(h,"CreateScratchImage");
    GetDataFn getData=(GetDataFn)GETSYM(h,"GetScratchImageData");
    SaveFn save=(SaveFn)GETSYM(h,"Save");
    DestroyFn destroy=(DestroyFn)GETSYM(h,"DestroyScratchImage");
    if(!create||!getData||!save||!destroy){fprintf(stderr,"missing export\n");return 3;}

    int W=16,H=16,fmt=atoi(argv[3]);
    void* img=0;
    HRESULT hr=create(W,H,28/*R8G8B8A8_UNORM*/,1,1,0,&img); /* scratch is always RGBA8 */
    if(hr||!img){fprintf(stderr,"create hr=%08x\n",hr);return 4;}
    struct ScratchImageData d; memset(&d,0,sizeof d);
    hr=getData(img,0,0,0,&d);
    if(hr){fprintf(stderr,"getData hr=%08x\n",hr);return 5;}
    for(int y=0;y<H;y++)for(int x=0;x<W;x++){ uint8_t*p=d.pixels+y*d.stride+x*4; p[0]=(uint8_t)((x+y)*8); p[1]=(uint8_t)(y*16); p[2]=(uint8_t)(x*16); p[3]=255; }

    struct DDSSaveInfo si; memset(&si,0,sizeof si);
    si.format=fmt; si.errorMetric=0/*Perceptual*/; si.compressionSpeed=1/*Medium*/; si.errorDiffusionDithering=1;
    struct IOCallbacks cb={ioRead,ioWrite,ioSeek,ioGetSize};
    g_pos=g_len=0;
    hr=save(&si,img,&cb,0/*no GPU adapter -> CPU*/,0/*no progress*/);
    if(hr){fprintf(stderr,"save hr=%08x\n",hr);return 6;}
    destroy(img);
    FILE*f=fopen(argv[2],"wb"); fwrite(g_buf,1,g_len,f); fclose(f);
    fprintf(stderr,"wrote %lld bytes\n",(long long)g_len); return 0;
}
