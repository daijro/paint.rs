// AES for plug-ins in the browser, whose runtime has no AES: its Aes.Create() returns an object whose encryptor and
// decryptor throw there. PolyGlitch's Codebook scrambles an image by encrypting its pixels in ECB mode, so the
// rewritten plug-in gets this one instead (PluginRewrite). Written from FIPS 197 (the cipher) and NIST SP 800-38A
// (the modes), and behaving as .NET's own Aes does: ECB, CBC and CFB with 8- or 128-bit feedback; PKCS7, zeros,
// ANSI X9.23, ISO 10126 or no padding; a decryptor that keeps its last block back until the final one, so it can
// take the padding off; and a transform that can be reused after its final block.
using System;
using System.Security.Cryptography;

namespace PaintDotNet.Bridge;

/// What a rewritten plug-in calls in place of Aes.Create().
public static class PluginCrypto
{
    public static Aes CreateAes() => new ManagedAes();
}

internal sealed class ManagedAes : Aes
{
    public override ICryptoTransform CreateEncryptor(byte[] rgbKey, byte[]? rgbIV) => new AesTransform(rgbKey, rgbIV, Mode, Padding, FeedbackSize, true);
    public override ICryptoTransform CreateDecryptor(byte[] rgbKey, byte[]? rgbIV) => new AesTransform(rgbKey, rgbIV, Mode, Padding, FeedbackSize, false);
    public override void GenerateKey() => KeyValue = RandomNumberGenerator.GetBytes(KeySizeValue / 8);
    public override void GenerateIV() => IVValue = RandomNumberGenerator.GetBytes(BlockSizeValue / 8);
}

internal sealed class AesTransform : ICryptoTransform
{
    private readonly uint[] keys;          // the round keys: the cipher's, or for ECB/CBC decryption the inverse cipher's
    private readonly int rounds;
    private readonly bool encrypt;
    private readonly CipherMode mode;
    private readonly PaddingMode padding;
    private readonly int unit;             // the bytes one step of the mode takes: 16, or 1 for 8-bit CFB
    private readonly byte[] iv = new byte[16];
    private readonly byte[] chain = new byte[16];
    private byte[]? held;                  // a decryptor's last block, until the next data or the final block

    public AesTransform(byte[] key, byte[]? iv, CipherMode mode, PaddingMode padding, int feedbackBits, bool encrypt)
    {
        ArgumentNullException.ThrowIfNull(key);
        if (key.Length is not (16 or 24 or 32)) throw new CryptographicException("Specified key is not a valid size for this algorithm.");
        if (mode is not (CipherMode.ECB or CipherMode.CBC or CipherMode.CFB)) throw new CryptographicException("Specified cipher mode is not valid for this algorithm.");
        if (mode == CipherMode.CFB && feedbackBits is not (8 or 128)) throw new CryptographicException("The specified feedback size is not valid for this algorithm.");
        if (mode != CipherMode.ECB)
        {
            if (iv is null) throw new CryptographicException("The cipher mode specified requires that an initialization vector (IV) be used.");
            if (iv.Length != 16) throw new CryptographicException("Specified initialization vector (IV) does not match the block size for this algorithm.");
            iv.CopyTo(this.iv, 0);
        }
        this.mode = mode;
        this.padding = padding;
        this.encrypt = encrypt;
        unit = mode == CipherMode.CFB ? feedbackBits / 8 : 16;
        rounds = key.Length / 4 + 6;
        // CFB runs the cipher forwards both ways; ECB and CBC decrypt with the inverse cipher.
        keys = encrypt || mode == CipherMode.CFB ? Expand(key) : Invert(Expand(key), rounds);
        this.iv.CopyTo(chain, 0);
    }

    public int InputBlockSize => unit;
    public int OutputBlockSize => unit;
    public bool CanTransformMultipleBlocks => true;
    public bool CanReuseTransform => true;
    public void Dispose() { }

    // Decrypting with padding that comes off (all but zeros and none), the last block waits for the final one.
    private bool Depads => !encrypt && padding is not (PaddingMode.None or PaddingMode.Zeros);

    public int TransformBlock(byte[] inputBuffer, int inputOffset, int inputCount, byte[] outputBuffer, int outputOffset)
    {
        ArgumentNullException.ThrowIfNull(inputBuffer);
        ArgumentNullException.ThrowIfNull(outputBuffer);
        if (inputCount <= 0 || inputCount % unit != 0 || inputOffset < 0 || inputCount > inputBuffer.Length - inputOffset)
            throw new ArgumentException("Value was invalid.", nameof(inputCount));
        if (!Depads)
        {
            Run(inputBuffer.AsSpan(inputOffset, inputCount), outputBuffer.AsSpan(outputOffset));
            return inputCount;
        }
        int written = 0;
        if (held is not null)
        {
            Run(held, outputBuffer.AsSpan(outputOffset));
            written = held.Length;
        }
        held = inputBuffer.AsSpan(inputOffset + inputCount - unit, unit).ToArray();
        Run(inputBuffer.AsSpan(inputOffset, inputCount - unit), outputBuffer.AsSpan(outputOffset + written));
        return written + inputCount - unit;
    }

    public byte[] TransformFinalBlock(byte[] inputBuffer, int inputOffset, int inputCount)
    {
        ArgumentNullException.ThrowIfNull(inputBuffer);
        if (inputOffset < 0 || inputCount < 0 || inputCount > inputBuffer.Length - inputOffset) throw new ArgumentException("Value was invalid.", nameof(inputCount));
        try { return encrypt ? EncryptFinal(inputBuffer.AsSpan(inputOffset, inputCount)) : DecryptFinal(inputBuffer.AsSpan(inputOffset, inputCount)); }
        finally
        {
            iv.CopyTo(chain, 0);
            held = null;
        }
    }

    private byte[] EncryptFinal(ReadOnlySpan<byte> data)
    {
        int rest = data.Length % unit;
        int pad = padding switch
        {
            PaddingMode.None when rest != 0 => throw new CryptographicException("The input data is not a complete block."),
            PaddingMode.None => 0,
            PaddingMode.Zeros => rest == 0 ? 0 : unit - rest,
            _ => unit - rest,
        };
        byte[] block = new byte[data.Length + pad];
        data.CopyTo(block);
        if (pad > 0)
            switch (padding)
            {
                case PaddingMode.PKCS7: block.AsSpan(data.Length).Fill((byte)pad); break;
                case PaddingMode.ANSIX923: block[^1] = (byte)pad; break;
                case PaddingMode.ISO10126:
                    RandomNumberGenerator.Fill(block.AsSpan(data.Length, pad - 1));
                    block[^1] = (byte)pad;
                    break;
            }
        Run(block, block);
        return block;
    }

    private byte[] DecryptFinal(ReadOnlySpan<byte> data)
    {
        byte[] block = new byte[(held?.Length ?? 0) + data.Length];
        held?.CopyTo(block, 0);
        data.CopyTo(block.AsSpan(held?.Length ?? 0));
        if (block.Length % unit != 0) throw new CryptographicException("The input data is not a complete block.");
        Run(block, block);
        if (!Depads || block.Length == 0) return block;
        int pad = block[^1];
        if (pad == 0 || pad > unit) throw new CryptographicException("Padding is invalid and cannot be removed.");
        for (int i = block.Length - pad; i < block.Length - 1; i++)
            if (padding == PaddingMode.PKCS7 && block[i] != pad || padding == PaddingMode.ANSIX923 && block[i] != 0)
                throw new CryptographicException("Padding is invalid and cannot be removed.");
        return block[..^pad];
    }

    // The mode over whole units of `src` into `dst` (which may be the same memory), carrying the chain across calls.
    private void Run(ReadOnlySpan<byte> src, Span<byte> dst)
    {
        Span<byte> block = stackalloc byte[16], cipherText = stackalloc byte[16];
        for (int at = 0; at < src.Length; at += unit)
        {
            ReadOnlySpan<byte> input = src.Slice(at, unit);
            Span<byte> output = dst.Slice(at, unit);
            switch (mode)
            {
                case CipherMode.ECB:
                    Cipher(input, output);
                    break;
                case CipherMode.CBC when encrypt:
                    for (int i = 0; i < 16; i++) block[i] = (byte)(input[i] ^ chain[i]);
                    Cipher(block, output);
                    output.CopyTo(chain);
                    break;
                case CipherMode.CBC:
                    input.CopyTo(block);
                    Cipher(block, output);
                    for (int i = 0; i < 16; i++) output[i] ^= chain[i];
                    block.CopyTo(chain);
                    break;
                default:
                    // CFB: the cipher of the last 16 ciphertext bytes, its first `unit` bytes XORed with the input;
                    // the ciphertext then shifts into the register.
                    Cipher(chain, block);
                    for (int i = 0; i < unit; i++)
                    {
                        byte c = encrypt ? (byte)(input[i] ^ block[i]) : input[i];
                        output[i] = (byte)(input[i] ^ block[i]);
                        cipherText[i] = c;
                    }
                    chain.AsSpan(unit).CopyTo(chain);
                    cipherText[..unit].CopyTo(chain.AsSpan(16 - unit));
                    break;
            }
        }
    }

    // One block through the cipher (FIPS 197 section 5.1) or, with the inverted keys, the equivalent inverse cipher
    // (section 5.3.5), as table lookups: each round's SubBytes, ShiftRows and MixColumns in one step per column.
    private void Cipher(ReadOnlySpan<byte> input, Span<byte> output)
    {
        bool inverse = !encrypt && mode != CipherMode.CFB;
        uint[] t0 = inverse ? Tables.D0 : Tables.E0, t1 = inverse ? Tables.D1 : Tables.E1,
               t2 = inverse ? Tables.D2 : Tables.E2, t3 = inverse ? Tables.D3 : Tables.E3;
        byte[] box = inverse ? Tables.InvSbox : Tables.Sbox;
        uint s0 = Word(input, 0) ^ keys[0], s1 = Word(input, 4) ^ keys[1], s2 = Word(input, 8) ^ keys[2], s3 = Word(input, 12) ^ keys[3];
        int k = 4;
        for (int r = 1; r < rounds; r++, k += 4)
        {
            uint a0, a1, a2, a3;
            if (inverse)
            {
                a0 = t0[s0 >> 24] ^ t1[(s3 >> 16) & 255] ^ t2[(s2 >> 8) & 255] ^ t3[s1 & 255] ^ keys[k];
                a1 = t0[s1 >> 24] ^ t1[(s0 >> 16) & 255] ^ t2[(s3 >> 8) & 255] ^ t3[s2 & 255] ^ keys[k + 1];
                a2 = t0[s2 >> 24] ^ t1[(s1 >> 16) & 255] ^ t2[(s0 >> 8) & 255] ^ t3[s3 & 255] ^ keys[k + 2];
                a3 = t0[s3 >> 24] ^ t1[(s2 >> 16) & 255] ^ t2[(s1 >> 8) & 255] ^ t3[s0 & 255] ^ keys[k + 3];
            }
            else
            {
                a0 = t0[s0 >> 24] ^ t1[(s1 >> 16) & 255] ^ t2[(s2 >> 8) & 255] ^ t3[s3 & 255] ^ keys[k];
                a1 = t0[s1 >> 24] ^ t1[(s2 >> 16) & 255] ^ t2[(s3 >> 8) & 255] ^ t3[s0 & 255] ^ keys[k + 1];
                a2 = t0[s2 >> 24] ^ t1[(s3 >> 16) & 255] ^ t2[(s0 >> 8) & 255] ^ t3[s1 & 255] ^ keys[k + 2];
                a3 = t0[s3 >> 24] ^ t1[(s0 >> 16) & 255] ^ t2[(s1 >> 8) & 255] ^ t3[s2 & 255] ^ keys[k + 3];
            }
            (s0, s1, s2, s3) = (a0, a1, a2, a3);
        }
        // The last round has no MixColumns: the S-box alone, with the rows shifted.
        uint Last(uint a, uint b, uint c, uint d, uint key) =>
            ((uint)box[a >> 24] << 24 | (uint)box[(b >> 16) & 255] << 16 | (uint)box[(c >> 8) & 255] << 8 | box[d & 255]) ^ key;
        uint o0, o1, o2, o3;
        if (inverse)
        {
            o0 = Last(s0, s3, s2, s1, keys[k]); o1 = Last(s1, s0, s3, s2, keys[k + 1]);
            o2 = Last(s2, s1, s0, s3, keys[k + 2]); o3 = Last(s3, s2, s1, s0, keys[k + 3]);
        }
        else
        {
            o0 = Last(s0, s1, s2, s3, keys[k]); o1 = Last(s1, s2, s3, s0, keys[k + 1]);
            o2 = Last(s2, s3, s0, s1, keys[k + 2]); o3 = Last(s3, s0, s1, s2, keys[k + 3]);
        }
        Put(output, 0, o0); Put(output, 4, o1); Put(output, 8, o2); Put(output, 12, o3);
    }

    private static uint Word(ReadOnlySpan<byte> b, int at) => (uint)(b[at] << 24 | b[at + 1] << 16 | b[at + 2] << 8 | b[at + 3]);

    private static void Put(Span<byte> b, int at, uint w)
    {
        b[at] = (byte)(w >> 24); b[at + 1] = (byte)(w >> 16); b[at + 2] = (byte)(w >> 8); b[at + 3] = (byte)w;
    }

    // The key schedule (FIPS 197 section 5.2): 4 * (rounds + 1) words.
    private static uint[] Expand(byte[] key)
    {
        int nk = key.Length / 4, total = 4 * (nk + 7);
        uint[] w = new uint[total];
        for (int i = 0; i < nk; i++) w[i] = Word(key, 4 * i);
        uint rcon = 1;
        for (int i = nk; i < total; i++)
        {
            uint t = w[i - 1];
            if (i % nk == 0)
            {
                t = Sub(t << 8 | t >> 24) ^ rcon << 24;
                rcon = Tables.Times2((byte)rcon);
            }
            else if (nk > 6 && i % nk == 4) t = Sub(t);
            w[i] = w[i - nk] ^ t;
        }
        return w;
    }

    private static uint Sub(uint w) =>
        (uint)Tables.Sbox[w >> 24] << 24 | (uint)Tables.Sbox[(w >> 16) & 255] << 16 | (uint)Tables.Sbox[(w >> 8) & 255] << 8 | Tables.Sbox[w & 255];

    // The equivalent inverse cipher's keys (FIPS 197 section 5.3.5): the rounds in reverse, the middle ones through
    // InvMixColumns (here D[S[x]], since each D table already holds the inverse S-box).
    private static uint[] Invert(uint[] w, int rounds)
    {
        uint[] d = new uint[w.Length];
        for (int r = 0; r <= rounds; r++)
            for (int j = 0; j < 4; j++)
            {
                uint x = w[4 * (rounds - r) + j];
                d[4 * r + j] = r == 0 || r == rounds ? x
                    : Tables.D0[Tables.Sbox[x >> 24]] ^ Tables.D1[Tables.Sbox[(x >> 16) & 255]]
                      ^ Tables.D2[Tables.Sbox[(x >> 8) & 255]] ^ Tables.D3[Tables.Sbox[x & 255]];
            }
        return d;
    }

    // The S-boxes and round tables, computed from the field arithmetic (FIPS 197 sections 4 and 5.1.1).
    private static class Tables
    {
        public static readonly byte[] Sbox = new byte[256], InvSbox = new byte[256];
        public static readonly uint[] E0 = new uint[256], E1 = new uint[256], E2 = new uint[256], E3 = new uint[256];
        public static readonly uint[] D0 = new uint[256], D1 = new uint[256], D2 = new uint[256], D3 = new uint[256];

        public static byte Times2(byte x) => (byte)(x << 1 ^ (x >> 7) * 0x1B);

        private static byte Mul(byte a, byte b)
        {
            byte p = 0;
            for (; b != 0; b >>= 1, a = Times2(a)) if ((b & 1) != 0) p ^= a;
            return p;
        }

        static Tables()
        {
            for (int x = 0; x < 256; x++)
            {
                // The multiplicative inverse (0 for 0), then the affine transformation.
                byte inv = 0;
                for (int y = 1; y < 256 && x != 0; y++) if (Mul((byte)x, (byte)y) == 1) { inv = (byte)y; break; }
                int s = inv;
                for (int i = 1; i < 5; i++) s ^= (inv << i | inv >> (8 - i)) & 255;
                Sbox[x] = (byte)(s ^ 0x63);
                InvSbox[Sbox[x]] = (byte)x;
            }
            for (int x = 0; x < 256; x++)
            {
                byte s = Sbox[x], si = InvSbox[x];
                uint e = (uint)Mul(s, 2) << 24 | (uint)s << 16 | (uint)s << 8 | Mul(s, 3);
                uint d = (uint)Mul(si, 14) << 24 | (uint)Mul(si, 9) << 16 | (uint)Mul(si, 13) << 8 | Mul(si, 11);
                (E0[x], E1[x], E2[x], E3[x]) = (e, e >> 8 | e << 24, e >> 16 | e << 16, e >> 24 | e << 8);
                (D0[x], D1[x], D2[x], D3[x]) = (d, d >> 8 | d << 24, d >> 16 | d << 16, d >> 24 | d << 8);
            }
        }
    }
}
