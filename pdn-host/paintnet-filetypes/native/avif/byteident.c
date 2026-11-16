// Byte-identity harness for the AVIF native helper. Calls CompressColorImage with a
// fixed 16x16 BGRA gradient + lossless identity-matrix options and writes the raw AV1
// OBU. Build + run against both the Linux .so and the Windows helper DLL under Wine,
// then compare (identical when both wrap libaom 3.14.1 — see build-deps.sh):
//   gcc -O2 byteident.c -ldl -o /tmp/bi && /tmp/bi ./libAvifNative_x64.so /tmp/ours.av1
//   x86_64-w64-mingw32-gcc -O2 byteident.c -o bi.exe
//   # with AvifNative_x64.dll + vcruntime140*.dll (from ref/pdn-wine) in the cwd:
//   WINEPREFIX=$HOME/.cache/pdnwine wine bi.exe AvifNative_x64.dll /tmp/win.av1
//   cmp /tmp/ours.av1 /tmp/win.av1   # -> identical
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
#endif
typedef struct { int32_t quality; int32_t encoderPreset; int32_t yuvFormat; int32_t maxThreads; uint8_t lossless; uint8_t losslessAlpha; } EncoderOptions;
typedef struct { uint16_t colorPrimaries; uint16_t transferCharacteristics; uint16_t matrixCoefficients; uint8_t fullRange; } CICPColorData;
typedef struct { uint8_t* scan0; uint32_t width, height, stride; int32_t pixelFormat; } BitmapData;
typedef uint8_t(*ProgressProc)(uint32_t,uint32_t);
typedef struct { ProgressProc cb; uint32_t done, total; } ProgressContext;
typedef void*(*AllocFn)(size_t);
typedef int32_t(*CompressFn)(const BitmapData*, const EncoderOptions*, ProgressContext*, const CICPColorData*, AllocFn, void**);
typedef const char*(*VerFn)(void);
static uint8_t g_buf[1<<20]; static size_t g_sz;
static uint8_t onProg(uint32_t d,uint32_t t){(void)d;(void)t;return 1;}
static void* onAlloc(size_t n){ g_sz=n; return g_buf; }
int main(int argc,char**argv){
    void* h=LOADLIB(argv[1]); if(!h){fprintf(stderr,"load fail %s\n",argv[1]);return 2;}
    VerFn ver=(VerFn)GETSYM(h,"GetAOMVersionString");
    CompressFn comp=(CompressFn)GETSYM(h,"CompressColorImage");
    if(!comp){fprintf(stderr,"no CompressColorImage\n");return 3;}
    fprintf(stderr,"aom=%s\n", ver?ver():"?");
    int W=16,H=16,stride=W*4; uint8_t* bgra=malloc((size_t)stride*H);
    for(int y=0;y<H;y++)for(int x=0;x<W;x++){uint8_t*p=bgra+y*stride+x*4;p[0]=x*16;p[1]=y*16;p[2]=(x+y)*8;p[3]=255;}
    BitmapData bd={bgra,(uint32_t)W,(uint32_t)H,(uint32_t)stride,0};
    EncoderOptions eo={0,3,4,1,1,1}; /* quality0, preset VerySlow(3), yuvFormat IdentityMatrix(4), maxThreads1, lossless, losslessAlpha */
    CICPColorData cicp={1,13,0,1}; /* BT709, Srgb, Identity, full */
    ProgressContext pc={onProg,0,0}; void* out=0; g_sz=0;
    int32_t st=comp(&bd,&eo,&pc,&cicp,onAlloc,&out);
    if(st!=0){fprintf(stderr,"compress status=%d\n",st);return 4;}
    FILE*f=fopen(argv[2],"wb"); fwrite(g_buf,1,g_sz,f); fclose(f);
    fprintf(stderr,"wrote %zu bytes\n",g_sz); return 0;
}
