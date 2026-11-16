// Drives libWebP_x64.so through the exact ABI WebPFileType.dll P/Invokes:
// encode a fixed BGRA image with the plugin's default options, then decode it
// back and check the round-trip. Writes the encoded bytes to argv[1] so the
// output can be diffed byte-for-byte against the official Windows helper.
//   cc selftest.c -o selftest && ./selftest out.webp
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef enum { Ok = 0 } WebPStatus; // only Ok==0 matters here
typedef struct { float quality; int effort; int preset; int lossless; } EncoderOptions; // bool is 1 byte; pad handled by compiler the same way as C# blittable
typedef void* (*CreateImageFn)(int w, int h, size_t* outSize, int* outStride);
typedef int (*SetDecoderMetadataFn)(const uint8_t* data, size_t size, int type);
typedef int (*WriteImageFn)(const uint8_t* image, size_t size);

extern int GetLibWebPVersion(void);
extern int WebPLoad(const uint8_t* data, size_t size, CreateImageFn, SetDecoderMetadataFn);
extern int WebPSave(WriteImageFn, const void* bitmap, int w, int h, int stride,
                    const EncoderOptions*, const void* metadata, void* progress);

// ---- save side ----
static uint8_t* g_enc; static size_t g_encSize;
static int onWrite(const uint8_t* img, size_t n) { g_enc = malloc(n); memcpy(g_enc, img, n); g_encSize = n; return Ok; }

// ---- load side ----
static uint8_t* g_dec; static size_t g_decSize; static int g_decStride, g_decW, g_decH;
static void* onCreate(int w, int h, size_t* outSize, int* outStride) {
    g_decW = w; g_decH = h; g_decStride = w * 4; *outStride = g_decStride;
    g_decSize = (size_t)g_decStride * h; *outSize = g_decSize;
    g_dec = malloc(g_decSize); return g_dec;
}
static int onMeta(const uint8_t* d, size_t n, int t) { (void)d;(void)n;(void)t; return 1; }

int main(int argc, char** argv) {
    int v = GetLibWebPVersion();
    printf("libwebp %d.%d.%d\n", (v>>16)&255, (v>>8)&255, v&255);

    const int W = 16, H = 16, stride = 16*4;
    uint8_t* bgra = malloc((size_t)stride*H);
    for (int y=0;y<H;y++) for (int x=0;x<W;x++) {
        uint8_t* p = bgra + y*stride + x*4;
        p[0]=(uint8_t)(x*16); p[1]=(uint8_t)(y*16); p[2]=(uint8_t)((x+y)*8); p[3]=255; // B,G,R,A
    }
    EncoderOptions opt; opt.quality=95.0f; opt.effort=7; opt.preset=2 /*WEBP_PRESET_PHOTO*/; opt.lossless=0;
    if (WebPSave(onWrite, bgra, W, H, stride, &opt, NULL, NULL) != Ok || !g_enc) { fprintf(stderr,"encode failed\n"); return 1; }
    printf("encoded %zu bytes, RIFF=%.4s WEBP=%.4s\n", g_encSize, g_enc, g_enc+8);

    if (WebPLoad(g_enc, g_encSize, onCreate, onMeta) != Ok || !g_dec) { fprintf(stderr,"decode failed\n"); return 1; }
    if (g_decW!=W || g_decH!=H) { fprintf(stderr,"size mismatch %dx%d\n", g_decW,g_decH); return 1; }
    printf("decoded %dx%d ok\n", g_decW, g_decH);

    if (argc>1) { FILE* f=fopen(argv[1],"wb"); fwrite(g_enc,1,g_encSize,f); fclose(f); printf("wrote %s\n", argv[1]); }
    return 0;
}
