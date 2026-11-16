// Byte-identity harness for the JPEG XL native helper. Calls SaveImage with a fixed
// 16x16 BGRA gradient + lossless options over an in-memory IO-callback buffer and writes
// the .jxl. Build + run against both the Linux .so and the Windows helper DLL under Wine
// (same invocation as native/avif/byteident.c). Result: BYTE-IDENTICAL to the bundled
// Windows helper (both libjxl 0.11.2) after the one RE'd 1.3.0 encoder delta — the sRGB
// rendering intent — was applied to the vendored wrapper (see JxlEncoder.cpp).
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
typedef struct { uint8_t* scan0; uint32_t width, height, stride; } BitmapData;
typedef struct { float distance; int32_t effort; uint8_t lossless; } EncoderOptions;
typedef struct { uint8_t* exif; size_t exifSize; uint8_t* icc; size_t iccSize; uint8_t* xmp; size_t xmpSize; } EncoderImageMetadata;
typedef int32_t(*WriteFn)(const uint8_t*, size_t);
typedef int32_t(*SeekFn)(uint64_t);
typedef struct { WriteFn Write; SeekFn Seek; } IOCallbacks;
typedef uint8_t(*ProgFn)(int32_t);
typedef int32_t(*SaveFn)(const BitmapData*, const EncoderOptions*, const EncoderImageMetadata*, IOCallbacks*, char*, ProgFn);
typedef uint32_t(*VerFn)(void);
static uint8_t g_buf[1<<20]; static size_t g_pos, g_max;
static int32_t onWrite(const uint8_t* b, size_t n){ memcpy(g_buf+g_pos,b,n); g_pos+=n; if(g_pos>g_max)g_max=g_pos; return 0; }
static int32_t onSeek(uint64_t p){ g_pos=(size_t)p; return 0; }
static uint8_t onProg(int32_t p){(void)p;return 1;}
int main(int argc,char**argv){
    void* h=LOADLIB(argv[1]); if(!h){fprintf(stderr,"load fail\n");return 2;}
    VerFn ver=(VerFn)GETSYM(h,"GetLibJxlVersion"); SaveFn save=(SaveFn)GETSYM(h,"SaveImage");
    if(!save){fprintf(stderr,"no SaveImage\n");return 3;}
    if(ver){uint32_t v=ver();fprintf(stderr,"libjxl %u.%u.%u\n",(v>>16)&0xff,(v>>8)&0xff,v&0xff);}
    int W=16,H=16,stride=W*4; uint8_t* bgra=malloc((size_t)stride*H);
    for(int y=0;y<H;y++)for(int x=0;x<W;x++){uint8_t*p=bgra+y*stride+x*4;p[0]=x*16;p[1]=y*16;p[2]=(x+y)*8;p[3]=255;}
    BitmapData bd={bgra,(uint32_t)W,(uint32_t)H,(uint32_t)stride};
    EncoderOptions eo={0.0f,7,1}; /* distance0, effort7, lossless */
    EncoderImageMetadata md; memset(&md,0,sizeof md);
    IOCallbacks cb={onWrite,onSeek}; char err[256]; memset(err,0,sizeof err);
    g_pos=g_max=0;
    int32_t st=save(&bd,&eo,&md,&cb,err,onProg);
    if(st!=0){fprintf(stderr,"save status=%d err=%s\n",st,err);return 4;}
    FILE*f=fopen(argv[2],"wb"); fwrite(g_buf,1,g_max,f); fclose(f);
    fprintf(stderr,"wrote %zu bytes\n",g_max); return 0;
}
